//! Adb falso para testes: faz o papel do binário e do servidor adb, dos
//! Dispositivos e do `scrcpy-server` 4.1 rodando neles. Os testes controlam o
//! que ele responde e conferem o que ele recebeu.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, mpsc};

use crate::adb::{AdbError, AdbLink, DeviceProcess, DeviceTracker};
use crate::device::Device;
use crate::session::SCRCPY_VERSION;

/// Um fluxo de vídeo real do `scrcpy-server` 4.1 (id do codec, pacotes de
/// sessão e de mídia), gravado de um Samsung SM-G9600 com
/// `examples/record_screen.rs` (`max_size=480`).
pub const SCREEN_FIXTURE: &[u8] = include_bytes!("../fixtures/screen-h264.bin");

pub struct FakeAdb {
    state: Arc<Mutex<State>>,
    tracker_connected: Notify,
}

struct State {
    /// `None` simula o binário do adb ausente.
    client_version: Option<u32>,
    missing_path: PathBuf,
    server_version: Option<u32>,
    start_failure: Option<String>,
    /// Versão do servidor que "outra ferramenta" sobe durante o start-server.
    foreign_server_on_start: Option<u32>,
    tracker: Option<mpsc::Sender<Result<Vec<Device>, AdbError>>>,
    devices: Vec<Device>,
    starts: u32,
    kills: u32,

    scrcpy_server: FakeScrcpyServer,
    push_failure: Option<String>,
    reverse_failure: Option<String>,
    pushes: Vec<Push>,
    /// Túneis reversos abertos: socket no Dispositivo → porta local.
    reverses: HashMap<String, u16>,
    /// Túneis diretos abertos: porta local → socket no Dispositivo.
    forwards: HashMap<u16, String>,
    /// O lado do Dispositivo de cada túnel direto, até o servidor aceitar.
    forward_listeners: HashMap<String, TcpListener>,
    shells: Vec<Vec<String>>,
    running_servers: usize,
}

/// Um `adb push` recebido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Push {
    pub serial: String,
    pub local: PathBuf,
    pub remote: String,
}

/// Como o `scrcpy-server` falso se comporta ao ser iniciado.
#[derive(Debug, Clone)]
pub struct FakeScrcpyServer {
    /// O nome enviado nos metadados do Dispositivo.
    pub device_name: String,
    /// O que vem depois dos metadados: id do codec, pacotes de sessão e de mídia.
    pub video: Vec<u8>,
    /// Termina antes de se conectar, com esta saída (ex.: Android antigo demais).
    pub exit_before_connecting: Option<String>,
    /// Inicia mas nunca conecta, como um servidor travado.
    pub never_connects: bool,
    /// Depois de enviar `video`, fecha a conexão e termina, como numa
    /// desconexão do cabo. Sem isso, espera o cliente fechar.
    pub hang_up_after_video: bool,
}

impl Default for FakeScrcpyServer {
    fn default() -> Self {
        Self {
            device_name: "SM-G9600".into(),
            video: SCREEN_FIXTURE.to_vec(),
            exit_before_connecting: None,
            never_connects: false,
            hang_up_after_video: false,
        }
    }
}

impl FakeAdb {
    /// Um adb da versão dada, sem servidor rodando.
    pub fn new(client_version: u32) -> Arc<Self> {
        Self::build(Some(client_version))
    }

    /// Nenhum binário do adb no caminho dado.
    pub fn missing(path: impl Into<PathBuf>) -> Arc<Self> {
        let fake = Self::build(None);
        fake.state().missing_path = path.into();
        fake
    }

    fn build(client_version: Option<u32>) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                client_version,
                missing_path: PathBuf::new(),
                server_version: None,
                start_failure: None,
                foreign_server_on_start: None,
                tracker: None,
                devices: vec![],
                starts: 0,
                kills: 0,
                scrcpy_server: FakeScrcpyServer::default(),
                push_failure: None,
                reverse_failure: None,
                pushes: vec![],
                reverses: HashMap::new(),
                forwards: HashMap::new(),
                forward_listeners: HashMap::new(),
                shells: vec![],
                running_servers: 0,
            })),
            tracker_connected: Notify::new(),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    /// Já existe um servidor adb dessa versão rodando.
    pub fn with_running_server(self: Arc<Self>, version: u32) -> Arc<Self> {
        self.state().server_version = Some(version);
        self
    }

    pub fn fail_start(&self, detail: &str) {
        self.state().start_failure = Some(detail.to_owned());
    }

    /// Simula outra ferramenta subindo o servidor dela, de outra versão,
    /// entre a checagem do Spiegel e o start-server dele.
    pub fn foreign_server_appears_on_start(&self, version: u32) {
        self.state().foreign_server_on_start = Some(version);
    }

    /// Troca a lista de Dispositivos e avisa quem está acompanhando. Espera
    /// alguém começar a acompanhar, se ninguém estiver.
    pub async fn set_devices(&self, devices: Vec<Device>) {
        self.state().devices = devices.clone();
        loop {
            let notified = self.tracker_connected.notified();
            let tracker = self.state().tracker.clone();
            if let Some(tracker) = tracker {
                if tracker.send(Ok(devices.clone())).await.is_ok() {
                    return;
                }
            }
            notified.await;
        }
    }

    /// O servidor adb morre: o acompanhamento fecha e não há servidor.
    pub fn kill_externally(&self) {
        let mut state = self.state();
        state.server_version = None;
        state.tracker = None;
    }

    pub fn starts(&self) -> u32 {
        self.state().starts
    }

    pub fn kills(&self) -> u32 {
        self.state().kills
    }

    /// Troca o comportamento do `scrcpy-server` nos próximos inícios.
    pub fn set_scrcpy_server(&self, server: FakeScrcpyServer) {
        self.state().scrcpy_server = server;
    }

    pub fn fail_push(&self, detail: &str) {
        self.state().push_failure = Some(detail.to_owned());
    }

    /// O `adb reverse` falha, como numa conexão por `adb connect` antiga.
    pub fn fail_reverse(&self, detail: &str) {
        self.state().reverse_failure = Some(detail.to_owned());
    }

    pub fn pushes(&self) -> Vec<Push> {
        self.state().pushes.clone()
    }

    /// Os argumentos de cada `adb shell` recebido, em ordem.
    pub fn shell_commands(&self) -> Vec<Vec<String>> {
        self.state().shells.clone()
    }

    /// Os túneis ainda abertos, reversos e diretos, pelo socket no Dispositivo.
    pub fn open_tunnels(&self) -> Vec<String> {
        let state = self.state();
        state.reverses.keys().chain(state.forwards.values()).cloned().collect()
    }

    /// Quantos `scrcpy-server` ainda estão rodando.
    pub fn running_servers(&self) -> usize {
        self.state().running_servers
    }
}

impl AdbLink for FakeAdb {
    async fn client_version(&self) -> Result<u32, AdbError> {
        let state = self.state();
        state.client_version.ok_or_else(|| AdbError::NotFound(state.missing_path.clone()))
    }

    async fn server_version(&self) -> Result<Option<u32>, AdbError> {
        Ok(self.state().server_version)
    }

    async fn start_server(&self) -> Result<(), AdbError> {
        let mut state = self.state();
        state.starts += 1;
        if let Some(detail) = state.start_failure.clone() {
            return Err(AdbError::CommandFailed(detail));
        }
        state.server_version = state.foreign_server_on_start.take().or(state.client_version);
        Ok(())
    }

    async fn kill_server(&self) -> Result<(), AdbError> {
        let mut state = self.state();
        state.kills += 1;
        state.server_version = None;
        state.tracker = None;
        Ok(())
    }

    async fn track_devices(&self) -> Result<DeviceTracker, AdbError> {
        let (tx, rx) = mpsc::channel(8);
        {
            let mut state = self.state();
            if state.server_version.is_none() {
                return Err(AdbError::Io(std::io::ErrorKind::ConnectionRefused.into()));
            }
            // Como o adb real, a lista atual chega logo de cara.
            tx.try_send(Ok(state.devices.clone())).expect("canal novo tem espaço");
            state.tracker = Some(tx);
        }
        self.tracker_connected.notify_waiters();
        Ok(rx)
    }

    async fn push(&self, serial: &str, local: &Path, remote: &str) -> Result<(), AdbError> {
        let mut state = self.state();
        if let Some(detail) = state.push_failure.clone() {
            return Err(AdbError::CommandFailed(detail));
        }
        state.pushes.push(Push { serial: serial.into(), local: local.into(), remote: remote.into() });
        Ok(())
    }

    async fn reverse(&self, _serial: &str, device_socket: &str, local_port: u16) -> Result<(), AdbError> {
        let mut state = self.state();
        if let Some(detail) = state.reverse_failure.clone() {
            return Err(AdbError::CommandFailed(detail));
        }
        state.reverses.insert(device_socket.into(), local_port);
        Ok(())
    }

    async fn remove_reverse(&self, _serial: &str, device_socket: &str) -> Result<(), AdbError> {
        match self.state().reverses.remove(device_socket) {
            Some(_) => Ok(()),
            None => Err(AdbError::CommandFailed(format!("listener '{device_socket}' not found"))),
        }
    }

    async fn forward(&self, _serial: &str, device_socket: &str) -> Result<u16, AdbError> {
        // O lado do Dispositivo é um socket de verdade na rede local, que o
        // servidor falso aceita quando inicia.
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let mut state = self.state();
        state.forwards.insert(port, device_socket.into());
        state.forward_listeners.insert(device_socket.into(), listener);
        Ok(port)
    }

    async fn remove_forward(&self, _serial: &str, local_port: u16) -> Result<(), AdbError> {
        let mut state = self.state();
        match state.forwards.remove(&local_port) {
            Some(socket) => {
                state.forward_listeners.remove(&socket);
                Ok(())
            }
            None => Err(AdbError::CommandFailed(format!("listener 'tcp:{local_port}' not found"))),
        }
    }

    async fn spawn_shell(&self, _serial: &str, args: &[String]) -> Result<DeviceProcess, AdbError> {
        let (process, mut control) = DeviceProcess::new();
        let launch = {
            let mut state = self.state();
            state.shells.push(args.to_vec());
            state.running_servers += 1;
            Launch::parse(args, &mut state)
        };
        let state = self.state.clone();
        tokio::spawn(async move {
            let output = tokio::select! {
                output = launch.run() => output,
                () = control.kill_requested() => "[server] killed".to_owned(),
            };
            state.lock().unwrap().running_servers -= 1;
            control.exited(output);
        });
        Ok(process)
    }
}

/// Um início do `scrcpy-server` falso, com o que ele leu dos argumentos.
struct Launch {
    server: FakeScrcpyServer,
    version: Option<String>,
    tunnel: Option<Tunnel>,
}

enum Tunnel {
    /// O servidor conecta na porta do túnel reverso.
    Connect(u16),
    /// O servidor aceita no túnel direto e envia o byte fictício.
    Accept(TcpListener),
}

impl Launch {
    /// Lê `CLASSPATH=… app_process / com.genymobile.scrcpy.Server <versão> chave=valor…`.
    fn parse(args: &[String], state: &mut State) -> Self {
        let server_class = args.iter().position(|arg| arg == "com.genymobile.scrcpy.Server");
        let version = server_class.and_then(|at| args.get(at + 1)).cloned();
        let option = |key: &str| {
            args.iter().find_map(|arg| arg.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
        };
        let socket = format!("localabstract:scrcpy_{}", option("scid").unwrap_or_default());
        let tunnel = if option("tunnel_forward") == Some("true") {
            state.forward_listeners.remove(&socket).map(Tunnel::Accept)
        } else {
            state.reverses.get(&socket).copied().map(Tunnel::Connect)
        };
        Self { server: state.scrcpy_server.clone(), version, tunnel }
    }

    /// Faz o papel do servidor e devolve a saída dele.
    async fn run(self) -> String {
        let version = self.version.unwrap_or_default();
        if version != SCRCPY_VERSION {
            return format!(
                "[server] ERROR: The server version ({SCRCPY_VERSION}) does not match the client ({version})"
            );
        }
        if let Some(output) = self.server.exit_before_connecting {
            return output;
        }
        if self.server.never_connects {
            std::future::pending::<()>().await;
        }
        let socket = match self.tunnel {
            Some(Tunnel::Connect(port)) => TcpStream::connect(("127.0.0.1", port)).await,
            Some(Tunnel::Accept(listener)) => match listener.accept().await {
                Ok((mut socket, _)) => socket.write_all(&[0]).await.map(|()| socket),
                Err(err) => Err(err),
            },
            None => return "[server] ERROR: could not connect to the tunnel".to_owned(),
        };
        let Ok(mut socket) = socket else {
            return "[server] ERROR: could not connect to the tunnel".to_owned();
        };

        let mut meta = [0u8; 64];
        let name = self.server.device_name.as_bytes();
        let len = name.len().min(63);
        meta[..len].copy_from_slice(&name[..len]);
        if socket.write_all(&meta).await.is_err() || socket.write_all(&self.server.video).await.is_err() {
            return String::new();
        }
        if !self.server.hang_up_after_video {
            // Como o servidor real, só termina quando o cliente fecha.
            let mut buf = [0u8; 64];
            while matches!(socket.read(&mut buf).await, Ok(n) if n > 0) {}
        }
        String::new()
    }
}
