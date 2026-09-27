//! Sessões: o `scrcpy-server` 4.1 rodando no Dispositivo e o fluxo de vídeo
//! dele chegando ao Spiegel.
//!
//! [`Session::start`] faz a inicialização inteira em segundo plano (envio do
//! servidor, túnel adb, início do servidor, sockets e metadados) e publica
//! tudo como [`SessionEvent`]s, até o [`SessionEvent::Ended`] final, que só
//! chega depois que tudo o que a Sessão montou no Dispositivo foi desfeito.

mod video;

use std::convert::Infallible;
use std::future::Future;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::adb::{AdbError, AdbLink, DeviceProcess};
use crate::registry::AdbProblem;

pub use video::{VideoCodec, VideoPacket};

/// Versão do `scrcpy-server` embutido. O servidor exige a string exata.
pub const SCRCPY_VERSION: &str = "4.1";

/// Para onde o servidor é enviado no Dispositivo, o mesmo caminho do scrcpy.
pub const DEVICE_SERVER_PATH: &str = "/data/local/tmp/scrcpy-server.jar";

/// Tamanho do campo com o nome do Dispositivo nos metadados.
const DEVICE_NAME_LENGTH: usize = 64;

/// Quanto o servidor tem para terminar sozinho depois que os sockets
/// fecham, antes de ser encerrado à força (o mesmo prazo do scrcpy).
const SERVER_EXIT_GRACE: Duration = Duration::from_secs(1);

/// Quanto esperar pela conexão depois que o servidor terminou.
const LATE_CONNECTION_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// O `scrcpy-server` 4.1 no computador.
    pub server_path: PathBuf,
    /// Prazo para o servidor iniciar e conectar os sockets.
    pub connect_timeout: Duration,
}

impl SessionOptions {
    pub fn new(server_path: PathBuf) -> Self {
        Self { server_path, connect_timeout: Duration::from_secs(10) }
    }
}

/// O fluxo de eventos de uma Sessão. Termina depois do [`SessionEvent::Ended`].
pub type SessionEvents = mpsc::Receiver<SessionEvent>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEvent {
    Phase { phase: StartupPhase },
    /// Os sockets conectaram. Falta a primeira imagem.
    #[serde(rename_all = "camelCase")]
    Connected { device_name: String },
    /// Uma nova captura começou (no início e a cada rotação): o
    /// decodificador precisa ser (re)configurado para este tamanho.
    VideoConfigured { codec: VideoCodec, width: u32, height: u32 },
    /// Um pacote do fluxo codificado, intacto. Não vai para a interface como
    /// JSON: a casca o envia em binário.
    #[serde(skip)]
    VideoPacket(VideoPacket),
    Ended { reason: EndReason },
}

/// Fases da inicialização, na ordem em que acontecem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupPhase {
    PushingServer,
    Connecting,
}

/// Por que a Sessão terminou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EndReason {
    /// Parada pelo usuário.
    Stopped,
    /// Um comando do adb falhou (envio do servidor, túnel ou shell).
    AdbFailed { problem: AdbProblem },
    /// O servidor terminou antes de conectar. `output` é a saída dele, que
    /// costuma explicar o motivo (ex.: Android antigo demais).
    ServerExited { output: String },
    /// O servidor não conectou dentro do prazo.
    ConnectTimeout,
    /// Não deu para abrir a conexão local com o servidor.
    ConnectionFailed { detail: String },
    /// O fluxo de vídeo fechou: o Dispositivo foi desconectado ou o servidor parou.
    Disconnected,
    /// O servidor enviou algo que não segue o protocolo 4.1.
    ProtocolError { detail: String },
}

/// Uma Sessão em execução. Descartá-la sem [`Session::stop`] também a
/// encerra, mas sem esperar a desmontagem terminar.
pub struct Session {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl Session {
    /// Inicia uma Sessão de Tela no Dispositivo. Precisa de um runtime tokio.
    pub fn start<L: AdbLink>(link: Arc<L>, serial: impl Into<String>, options: SessionOptions) -> (Self, SessionEvents) {
        let (events_tx, events) = mpsc::channel(64);
        let (stop, stop_rx) = oneshot::channel();
        let runner = Runner { link, serial: serial.into(), options, events: events_tx, tunnel: None, process: None };
        let task = tokio::spawn(runner.run(stop_rx));
        (Self { stop: Some(stop), task }, events)
    }

    /// Para a Sessão e espera a desmontagem: sockets fechados, servidor
    /// encerrado e túnel removido.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = (&mut self.task).await;
    }
}

enum Tunnel {
    Reverse(String),
    Forward(u16),
}

struct Runner<L> {
    link: Arc<L>,
    serial: String,
    options: SessionOptions,
    events: mpsc::Sender<SessionEvent>,
    /// O que a Sessão montou no Dispositivo e precisa desfazer.
    tunnel: Option<Tunnel>,
    process: Option<DeviceProcess>,
}

impl<L: AdbLink> Runner<L> {
    async fn run(mut self, mut stop: oneshot::Receiver<()>) {
        // Descartar o `Session` fecha `stop`, o que também encerra.
        let reason = tokio::select! {
            _ = &mut stop => EndReason::Stopped,
            Err(reason) = self.drive() => reason,
        };
        // Os sockets viviam dentro de `drive` e já fecharam aqui.
        self.teardown().await;
        let _ = self.events.send(SessionEvent::Ended { reason }).await;
    }

    async fn drive(&mut self) -> Result<Infallible, EndReason> {
        self.emit(SessionEvent::Phase { phase: StartupPhase::PushingServer }).await?;
        self.link.push(&self.serial, &self.options.server_path, DEVICE_SERVER_PATH).await.map_err(adb_failed)?;

        self.emit(SessionEvent::Phase { phase: StartupPhase::Connecting }).await?;
        let (video, device_name) = self.connect().await?;
        // Como no scrcpy: com os sockets conectados, o túnel não serve mais.
        self.remove_tunnel().await;
        self.emit(SessionEvent::Connected { device_name }).await?;

        let events = self.events.clone();
        let reason = video::forward_stream(video, |event| {
            let events = events.clone();
            async move { events.send(event).await.is_ok() }
        })
        .await;
        Err(reason)
    }

    /// Abre o túnel (reverso, ou direto se o reverso falhar), inicia o
    /// servidor e espera o socket de vídeo com os metadados.
    async fn connect(&mut self) -> Result<(TcpStream, String), EndReason> {
        let scid = random_scid();
        let device_socket = format!("localabstract:scrcpy_{scid:08x}");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.map_err(connection_failed)?;
        let port = listener.local_addr().map_err(connection_failed)?.port();

        // Registrado antes do `await`: se a Sessão for parada no meio do
        // comando, a desmontagem ainda tenta remover o túnel.
        self.tunnel = Some(Tunnel::Reverse(device_socket.clone()));
        if self.link.reverse(&self.serial, &device_socket, port).await.is_ok() {
            self.start_server(scid, false).await?;
            self.until_connected(async move {
                let (mut socket, _) = listener.accept().await?;
                let name = read_device_name(&mut socket).await?;
                Ok((socket, name))
            })
            .await
        } else {
            // O reverso falha, por exemplo, em algumas conexões por rede.
            drop(listener);
            self.tunnel = None;
            let port = self.link.forward(&self.serial, &device_socket).await.map_err(adb_failed)?;
            self.tunnel = Some(Tunnel::Forward(port));
            self.start_server(scid, true).await?;
            self.until_connected(async move {
                let mut socket = connect_forward(port).await;
                let name = read_device_name(&mut socket).await?;
                Ok((socket, name))
            })
            .await
        }
    }

    async fn start_server(&mut self, scid: u32, tunnel_forward: bool) -> Result<(), EndReason> {
        let args = server_args(scid, tunnel_forward);
        self.process = Some(self.link.spawn_shell(&self.serial, &args).await.map_err(adb_failed)?);
        Ok(())
    }

    /// Espera a conexão, a menos que o servidor termine ou o prazo acabe antes.
    async fn until_connected<T>(
        &mut self,
        connect: impl Future<Output = std::io::Result<T>>,
    ) -> Result<T, EndReason> {
        let process = self.process.as_mut().expect("o servidor foi iniciado antes");
        let connect = tokio::time::timeout(self.options.connect_timeout, connect);
        tokio::pin!(connect);
        let connected = tokio::select! {
            connected = &mut connect => connected,
            exit = process.exited() => {
                // O que o servidor enviou antes de sair ainda pode estar a
                // caminho pelo túnel; só é falha se não chegar.
                match tokio::time::timeout(LATE_CONNECTION_GRACE, &mut connect).await {
                    Ok(Ok(Ok(value))) => Ok(Ok(value)),
                    _ => return Err(EndReason::ServerExited { output: exit.output }),
                }
            }
        };
        match connected {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => Err(connection_failed(err)),
            Err(_) => Err(EndReason::ConnectTimeout),
        }
    }

    async fn emit(&self, event: SessionEvent) -> Result<(), EndReason> {
        // Ninguém mais escuta: não há por que continuar.
        self.events.send(event).await.map_err(|_| EndReason::Stopped)
    }

    async fn remove_tunnel(&mut self) {
        // Uma falha aqui não tem o que fazer além de seguir: o túnel também
        // some quando o Dispositivo desconecta ou o servidor adb reinicia.
        match self.tunnel.take() {
            Some(Tunnel::Reverse(socket)) => {
                let _ = self.link.remove_reverse(&self.serial, &socket).await;
            }
            Some(Tunnel::Forward(port)) => {
                let _ = self.link.remove_forward(&self.serial, port).await;
            }
            None => {}
        }
    }

    async fn teardown(&mut self) {
        self.remove_tunnel().await;
        if let Some(mut process) = self.process.take() {
            // Com os sockets fechados, o servidor sai sozinho. Alguns
            // Dispositivos dormindo não acordam para isso, então há um prazo.
            if tokio::time::timeout(SERVER_EXIT_GRACE, process.exited()).await.is_err() {
                process.kill();
                let _ = tokio::time::timeout(SERVER_EXIT_GRACE, process.exited()).await;
            }
        }
    }
}

/// Os argumentos do `adb shell` que iniciam o servidor. Só o que difere dos
/// padrões do servidor vai na linha, como no scrcpy.
fn server_args(scid: u32, tunnel_forward: bool) -> Vec<String> {
    let mut args = vec![
        format!("CLASSPATH={DEVICE_SERVER_PATH}"),
        "app_process".into(),
        "/".into(),
        "com.genymobile.scrcpy.Server".into(),
        SCRCPY_VERSION.into(),
        format!("scid={scid:08x}"),
        "log_level=info".into(),
        "audio=false".into(),
        // O Controle remoto chega nas próximas etapas (#7).
        "control=false".into(),
    ];
    if tunnel_forward {
        args.push("tunnel_forward=true".into());
    }
    args
}

/// Com o túnel direto, a conexão local sempre é aceita pelo adb, mesmo que
/// o servidor ainda não escute no Dispositivo. O byte fictício que o servidor
/// envia confirma que há alguém do outro lado. Tenta até conseguir; quem
/// chama impõe o prazo.
async fn connect_forward(port: u16) -> TcpStream {
    loop {
        if let Ok(mut socket) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await {
            let mut dummy = [0u8; 1];
            if matches!(socket.read(&mut dummy).await, Ok(1)) {
                return socket;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn read_device_name(socket: &mut TcpStream) -> std::io::Result<String> {
    let mut field = [0u8; DEVICE_NAME_LENGTH];
    socket.read_exact(&mut field).await?;
    let len = field.iter().position(|&byte| byte == 0).unwrap_or(field.len());
    Ok(String::from_utf8_lossy(&field[..len]).into_owned())
}

/// Um id de 31 bits para os nomes do socket e do túnel, para que Sessões
/// iniciadas ao mesmo tempo no mesmo Dispositivo não colidam.
fn random_scid() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    hasher.finish() as u32 & 0x7fff_ffff
}

fn adb_failed(err: AdbError) -> EndReason {
    EndReason::AdbFailed { problem: (&err).into() }
}

fn connection_failed(err: std::io::Error) -> EndReason {
    EndReason::ConnectionFailed { detail: err.to_string() }
}
