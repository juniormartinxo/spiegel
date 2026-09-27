//! Acesso ao adb.
//!
//! [`AdbLink`] é o ponto de substituição interno da spec (#4): a versão real
//! ([`server_link::AdbServerLink`]) fala com o servidor adb, e nos testes entra
//! o adb falso (`testing::FakeAdb`).

pub mod protocol;
pub mod server_link;

use std::future::Future;
use std::path::PathBuf;

use tokio::sync::mpsc;

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
}

#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    #[error("binário do adb não encontrado em {0}")]
    NotFound(PathBuf),
    #[error("falha ao executar o adb: {0}")]
    Spawn(std::io::Error),
    #[error("o adb terminou com erro: {0}")]
    Failed(String),
    #[error("erro de comunicação com o servidor adb: {0}")]
    Io(#[from] std::io::Error),
    #[error("resposta inesperada do servidor adb: {0}")]
    Protocol(String),
    #[error("o servidor adb recusou o pedido: {0}")]
    Rejected(String),
}
