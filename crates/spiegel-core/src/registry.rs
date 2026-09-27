//! Registro de Dispositivos: mantém a lista ao vivo e o estado do adb.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, watch};
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
    /// espera o usuário decidir ([`DeviceRegistry::restart_server`]).
    #[serde(rename_all = "camelCase")]
    Conflict { server_version: u32, client_version: u32 },
    /// O servidor adb sumiu. O registro tenta de novo sozinho.
    Reconnecting,
    Unavailable { problem: AdbProblem },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AdbProblem {
    /// O binário do adb não existe no caminho configurado.
    NotFound { path: String },
    /// O adb existe mas falhou (não executa, não inicia o servidor…).
    Failed { detail: String },
}

impl From<&AdbError> for AdbProblem {
    fn from(err: &AdbError) -> Self {
        match err {
            AdbError::NotFound(path) => Self::NotFound { path: path.display().to_string() },
            other => Self::Failed { detail: other.to_string() },
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

enum Command {
    RestartServer,
}

/// Acompanha os Dispositivos enquanto existir. Precisa de um runtime tokio.
pub struct DeviceRegistry {
    snapshot: watch::Receiver<RegistrySnapshot>,
    commands: mpsc::Sender<Command>,
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

    /// Encerra o servidor adb em execução e inicia um com o adb do Spiegel.
    /// Só deve ser chamado por decisão do usuário (ex.: ao resolver um
    /// [`AdbStatus::Conflict`]).
    pub async fn restart_server(&self) {
        self.restart_handle().restart_server().await;
    }

    /// Um handle clonável para pedir [`Self::restart_server`] sem manter uma
    /// referência ao registro.
    pub fn restart_handle(&self) -> RestartHandle {
        RestartHandle { commands: self.commands.clone() }
    }
}

#[derive(Clone)]
pub struct RestartHandle {
    commands: mpsc::Sender<Command>,
}

impl RestartHandle {
    pub async fn restart_server(&self) {
        let _ = self.commands.send(Command::RestartServer).await;
    }
}

impl Drop for DeviceRegistry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Worker<L> {
    link: Arc<L>,
    options: RegistryOptions,
    snapshot: watch::Sender<RegistrySnapshot>,
    commands: mpsc::Receiver<Command>,
}

/// Por que um ciclo de acompanhamento terminou.
enum Outcome {
    Retry,
    RestartRequested,
}

impl<L: AdbLink> Worker<L> {
    async fn run(mut self) {
        let client_version = loop {
            match self.link.client_version().await {
                Ok(version) => break version,
                Err(err) => {
                    self.set(AdbStatus::Unavailable { problem: (&err).into() }, vec![]);
                    // Sem binário não há o que tentar sozinho; um pedido do
                    // usuário (reiniciar) faz tentar de novo.
                    if self.commands.recv().await.is_none() {
                        return;
                    }
                }
            }
        };

        let mut restart = false;
        loop {
            let outcome = self.track_once(client_version, restart).await;
            restart = matches!(outcome, Outcome::RestartRequested);
            if !restart && self.wait_or_command().await {
                restart = true;
            }
        }
    }

    async fn track_once(&mut self, client_version: u32, restart: bool) -> Outcome {
        if restart {
            if let Err(err) = self.link.kill_server().await {
                self.set(AdbStatus::Unavailable { problem: (&err).into() }, vec![]);
                return Outcome::Retry;
            }
        }

        let status = match self.link.server_version().await {
            Ok(Some(version)) if version == client_version => AdbStatus::Ready { version },
            Ok(Some(server_version)) => AdbStatus::Conflict { server_version, client_version },
            Ok(None) => match self.link.start_server().await {
                Ok(()) => AdbStatus::Ready { version: client_version },
                Err(err) => {
                    self.set(AdbStatus::Unavailable { problem: (&err).into() }, vec![]);
                    return Outcome::Retry;
                }
            },
            Err(err) => {
                self.set(AdbStatus::Unavailable { problem: (&err).into() }, vec![]);
                return Outcome::Retry;
            }
        };

        let mut tracker = match self.link.track_devices().await {
            Ok(tracker) => tracker,
            Err(err) => {
                self.set(AdbStatus::Unavailable { problem: (&err).into() }, vec![]);
                return Outcome::Retry;
            }
        };
        self.set(status.clone(), vec![]);

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
                    Some(Command::RestartServer) => return Outcome::RestartRequested,
                    None => return Outcome::Retry,
                },
            }
        }
    }

    /// Espera o intervalo de nova tentativa. `true` se o usuário pediu para
    /// reiniciar o servidor nesse meio-tempo.
    async fn wait_or_command(&mut self) -> bool {
        tokio::select! {
            () = tokio::time::sleep(self.options.retry_delay) => false,
            command = self.commands.recv() => matches!(command, Some(Command::RestartServer)),
        }
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
