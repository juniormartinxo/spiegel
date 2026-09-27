//! Registro de Dispositivos: mantém a lista ao vivo e o estado do adb.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::adb::{AdbError, AdbLink};
use crate::device::Device;

/// O que a interface mostra: o estado do adb e os Dispositivos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySnapshot {
    pub adb: AdbStatus,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AdbStatus {
    Starting,
    Ready { version: u32 },
    /// Já havia um servidor adb de outra versão rodando. O Spiegel não o
    /// derruba sozinho: continua acompanhando os Dispositivos por ele e
    /// espera o usuário decidir ([`RestartHandle::restart_server`]).
    #[serde(rename_all = "camelCase")]
    Conflict { server_version: u32, client_version: u32 },
    /// O servidor adb sumiu. O registro tenta de novo sozinho.
    Reconnecting,
    Unavailable { problem: AdbProblem },
}

/// Por que o adb está indisponível. A interface traduz o `kind`; o `detail`
/// é o texto técnico cru (mensagem do sistema, saída do adb), sem tradução.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AdbProblem {
    /// O binário do adb não existe no caminho configurado.
    NotFound { path: String },
    /// O binário existe mas não pôde ser executado.
    SpawnFailed { detail: String },
    /// O adb executou e terminou com erro (ex.: `start-server` falhou).
    CommandFailed { detail: String },
    /// O adb respondeu algo que o Spiegel não entende.
    UnexpectedOutput { detail: String },
    /// Não deu para conversar com o servidor adb pela rede local.
    ConnectionFailed { detail: String },
    /// O servidor adb recusou um pedido.
    Rejected { detail: String },
    /// O servidor adb não respondeu a tempo.
    Timeout { detail: String },
}

impl From<&AdbError> for AdbProblem {
    fn from(err: &AdbError) -> Self {
        match err {
            AdbError::NotFound(path) => Self::NotFound { path: path.display().to_string() },
            AdbError::Spawn(err) => Self::SpawnFailed { detail: err.to_string() },
            AdbError::CommandFailed(detail) => Self::CommandFailed { detail: detail.clone() },
            AdbError::UnexpectedOutput(detail) | AdbError::Protocol(detail) => {
                Self::UnexpectedOutput { detail: detail.clone() }
            }
            AdbError::Io(err) => Self::ConnectionFailed { detail: err.to_string() },
            AdbError::Rejected(detail) => Self::Rejected { detail: detail.clone() },
            AdbError::Timeout(detail) => Self::Timeout { detail: (*detail).to_owned() },
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegistryOptions {
    /// Espera entre tentativas quando o servidor adb some ou falha.
    pub retry_delay: Duration,
}

impl Default for RegistryOptions {
    fn default() -> Self {
        Self { retry_delay: Duration::from_secs(1) }
    }
}

/// Pedido de reinício, com o aviso de quando ele terminou.
type RestartRequest = oneshot::Sender<()>;

/// Acompanha os Dispositivos enquanto existir. Precisa de um runtime tokio.
pub struct DeviceRegistry {
    snapshot: watch::Receiver<RegistrySnapshot>,
    commands: mpsc::Sender<RestartRequest>,
    task: JoinHandle<()>,
}

impl DeviceRegistry {
    pub fn start<L: AdbLink>(link: Arc<L>, options: RegistryOptions) -> Self {
        let (snapshot_tx, snapshot) = watch::channel(RegistrySnapshot { adb: AdbStatus::Starting, devices: vec![] });
        let (commands, command_rx) = mpsc::channel(4);
        let task = tokio::spawn(Worker { link, options, snapshot: snapshot_tx, commands: command_rx }.run());
        Self { snapshot, commands, task }
    }

    pub fn snapshot(&self) -> RegistrySnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<RegistrySnapshot> {
        self.snapshot.clone()
    }

    /// Handle clonável para pedir o reinício do servidor adb sem manter uma
    /// referência ao registro.
    pub fn restart_handle(&self) -> RestartHandle {
        RestartHandle { commands: self.commands.clone() }
    }
}

impl Drop for DeviceRegistry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
pub struct RestartHandle {
    commands: mpsc::Sender<RestartRequest>,
}

impl RestartHandle {
    /// Encerra o servidor adb em execução e inicia um com o adb configurado.
    /// Só por decisão do usuário (ex.: ao resolver um
    /// [`AdbStatus::Conflict`]). Retorna quando o novo estado já foi
    /// publicado.
    pub async fn restart_server(&self) {
        let (done, finished) = oneshot::channel();
        if self.commands.send(done).await.is_ok() {
            let _ = finished.await;
        }
    }
}

struct Worker<L> {
    link: Arc<L>,
    options: RegistryOptions,
    snapshot: watch::Sender<RegistrySnapshot>,
    commands: mpsc::Receiver<RestartRequest>,
}

/// Por que um ciclo de acompanhamento terminou.
enum Outcome {
    Retry,
    Restart(RestartRequest),
}

impl<L: AdbLink> Worker<L> {
    async fn run(mut self) {
        let client_version = loop {
            match self.link.client_version().await {
                Ok(version) => break version,
                Err(err) => {
                    self.unavailable(&err);
                    // Sem binário não há o que tentar sozinho; um pedido do
                    // usuário (reiniciar) faz tentar de novo.
                    match self.commands.recv().await {
                        Some(done) => {
                            let _ = done.send(());
                        }
                        None => return,
                    }
                }
            }
        };

        let mut restart = None;
        loop {
            restart = match self.track_once(client_version, restart).await {
                Outcome::Restart(done) => Some(done),
                Outcome::Retry => self.wait_or_restart().await,
            };
        }
    }

    /// Um ciclo: garante um servidor, acompanha os Dispositivos até o
    /// servidor sumir ou o usuário pedir reinício. `restart` avisa quem
    /// pediu o reinício assim que o novo estado for publicado.
    async fn track_once(&mut self, client_version: u32, restart: Option<RestartRequest>) -> Outcome {
        let restarting = restart.is_some();
        let finish = restart.map(Finish);

        if restarting {
            if let Err(err) = self.link.kill_server().await {
                return self.unavailable(&err);
            }
        }

        let status = match self.link.server_version().await {
            Ok(Some(version)) if version == client_version => AdbStatus::Ready { version },
            Ok(Some(server_version)) => AdbStatus::Conflict { server_version, client_version },
            Ok(None) => match self.start_server(client_version).await {
                Ok(status) => status,
                Err(err) => return self.unavailable(&err),
            },
            Err(err) => return self.unavailable(&err),
        };

        let mut tracker = match self.link.track_devices().await {
            Ok(tracker) => tracker,
            Err(err) => return self.unavailable(&err),
        };
        self.set(status.clone(), vec![]);
        drop(finish);

        loop {
            tokio::select! {
                item = tracker.recv() => match item {
                    Some(Ok(devices)) => self.set(status.clone(), devices),
                    Some(Err(_)) | None => {
                        self.set(AdbStatus::Reconnecting, vec![]);
                        return Outcome::Retry;
                    }
                },
                command = self.commands.recv() => match command {
                    Some(done) => return Outcome::Restart(done),
                    None => return Outcome::Retry,
                },
            }
        }
    }

    /// Inicia o servidor e confere de novo a versão: se outra ferramenta
    /// subiu o servidor dela no meio-tempo, é um conflito, não sucesso.
    async fn start_server(&self, client_version: u32) -> Result<AdbStatus, AdbError> {
        self.link.start_server().await?;
        match self.link.server_version().await? {
            Some(version) if version == client_version => Ok(AdbStatus::Ready { version }),
            Some(server_version) => Ok(AdbStatus::Conflict { server_version, client_version }),
            None => Err(AdbError::Timeout("server not reachable after start-server")),
        }
    }

    /// Espera o intervalo de nova tentativa, ou um pedido de reinício.
    async fn wait_or_restart(&mut self) -> Option<RestartRequest> {
        tokio::select! {
            () = tokio::time::sleep(self.options.retry_delay) => None,
            command = self.commands.recv() => command,
        }
    }

    fn unavailable(&self, err: &AdbError) -> Outcome {
        self.set(AdbStatus::Unavailable { problem: err.into() }, vec![]);
        Outcome::Retry
    }

    fn set(&self, adb: AdbStatus, devices: Vec<Device>) {
        self.snapshot.send_if_modified(|current| {
            let next = RegistrySnapshot { adb, devices };
            let changed = *current != next;
            *current = next;
            changed
        });
    }
}

/// Avisa quem pediu o reinício quando sai de escopo, por qualquer caminho.
struct Finish(RestartRequest);

impl Drop for Finish {
    fn drop(&mut self) {
        // O oneshot só envia uma vez; troca por um canal morto para poder mover.
        let (dead, _) = oneshot::channel();
        let _ = std::mem::replace(&mut self.0, dead).send(());
    }
}
