//! Acesso ao adb.
//!
//! [`AdbLink`] é o ponto de substituição interno da spec (#4): a versão real
//! ([`server_link::AdbServerLink`]) fala com o servidor adb, e nos testes entra
//! o adb falso (`testing::FakeAdb`).

pub mod protocol;
pub mod server_link;

use std::future::Future;
use std::path::{Path, PathBuf};

use tokio::sync::{mpsc, oneshot, watch};

use crate::device::Device;

/// Fluxo de listas de Dispositivos: uma lista completa a cada mudança. O
/// canal fecha quando o servidor adb some.
pub type DeviceTracker = mpsc::Receiver<Result<Vec<Device>, AdbError>>;

/// Tudo o que o núcleo precisa do adb.
pub trait AdbLink: Send + Sync + 'static {
    /// Versão do protocolo do binário adb que o Spiegel usa (41 para o adb 1.0.41).
    fn client_version(&self) -> impl Future<Output = Result<u32, AdbError>> + Send;

    /// Versão do servidor adb em execução, ou `None` se nenhum estiver rodando.
    fn server_version(&self) -> impl Future<Output = Result<Option<u32>, AdbError>> + Send;

    /// Inicia o servidor adb com o binário do Spiegel.
    fn start_server(&self) -> impl Future<Output = Result<(), AdbError>> + Send;

    /// Encerra o servidor adb em execução, qualquer que seja a versão dele.
    fn kill_server(&self) -> impl Future<Output = Result<(), AdbError>> + Send;

    /// Começa a acompanhar os Dispositivos. A primeira lista chega logo.
    fn track_devices(&self) -> impl Future<Output = Result<DeviceTracker, AdbError>> + Send;

    /// Copia um arquivo do computador para o Dispositivo (`adb push`).
    fn push(&self, serial: &str, local: &Path, remote: &str) -> impl Future<Output = Result<(), AdbError>> + Send;

    /// Túnel reverso: as conexões do Dispositivo em `device_socket` (ex.:
    /// `localabstract:scrcpy_0123abcd`) chegam em `127.0.0.1:local_port`.
    fn reverse(
        &self,
        serial: &str,
        device_socket: &str,
        local_port: u16,
    ) -> impl Future<Output = Result<(), AdbError>> + Send;

    fn remove_reverse(&self, serial: &str, device_socket: &str) -> impl Future<Output = Result<(), AdbError>> + Send;

    /// Túnel direto: as conexões em `127.0.0.1:<porta>` chegam em
    /// `device_socket` no Dispositivo. O adb escolhe a porta e a devolve.
    fn forward(&self, serial: &str, device_socket: &str) -> impl Future<Output = Result<u16, AdbError>> + Send;

    fn remove_forward(&self, serial: &str, local_port: u16) -> impl Future<Output = Result<(), AdbError>> + Send;

    /// Roda um comando no shell do Dispositivo, sem esperar que ele termine.
    fn spawn_shell(
        &self,
        serial: &str,
        args: &[String],
    ) -> impl Future<Output = Result<DeviceProcess, AdbError>> + Send;
}

/// Um processo rodando no Dispositivo, iniciado por [`AdbLink::spawn_shell`].
/// Descartar o handle encerra o processo.
pub struct DeviceProcess {
    kill: Option<oneshot::Sender<()>>,
    exit: watch::Receiver<Option<ProcessExit>>,
}

/// Como o processo terminou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExit {
    /// A saída do processo (stdout e stderr juntos), para diagnóstico.
    pub output: String,
}

/// O lado de quem implementa o [`AdbLink`]: recebe o pedido de encerramento
/// e avisa quando o processo terminou.
pub struct ProcessControl {
    kill: oneshot::Receiver<()>,
    exit: watch::Sender<Option<ProcessExit>>,
}

impl ProcessControl {
    /// Resolve quando pedem para encerrar o processo ou quando o
    /// [`DeviceProcess`] é descartado.
    pub async fn kill_requested(&mut self) {
        let _ = (&mut self.kill).await;
    }

    /// Avisa que o processo terminou, com a saída dele.
    pub fn report_exit(self, output: String) {
        let _ = self.exit.send(Some(ProcessExit { output }));
    }
}

impl DeviceProcess {
    pub fn new() -> (Self, ProcessControl) {
        let (kill, kill_rx) = oneshot::channel();
        let (exit_tx, exit) = watch::channel(None);
        (Self { kill: Some(kill), exit }, ProcessControl { kill: kill_rx, exit: exit_tx })
    }

    /// Espera o processo terminar. Pode ser cancelado e chamado de novo.
    pub async fn exited(&mut self) -> ProcessExit {
        match self.exit.wait_for(Option::is_some).await {
            Ok(exit) => exit.clone().unwrap_or_else(|| unreachable!("wait_for garante Some")),
            // Quem implementa sumiu sem avisar: conta como terminado.
            Err(_) => ProcessExit { output: String::new() },
        }
    }

    /// Pede para encerrar o processo, sem esperar que ele termine.
    pub fn kill(&mut self) {
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
    }
}

/// Erros do adb. As mensagens são técnicas, em inglês, e servem para log.
/// A interface não as mostra: ela traduz o `AdbProblem` correspondente.
#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    #[error("adb binary not found at {0}")]
    NotFound(PathBuf),
    #[error("failed to run adb: {0}")]
    Spawn(std::io::Error),
    #[error("adb command failed: {0}")]
    CommandFailed(String),
    #[error("unexpected adb output: {0}")]
    UnexpectedOutput(String),
    #[error("adb server connection error: {0}")]
    Io(#[from] std::io::Error),
    #[error("adb protocol error: {0}")]
    Protocol(String),
    #[error("adb server rejected the request: {0}")]
    Rejected(String),
    #[error("adb timeout: {0}")]
    Timeout(&'static str),
}
