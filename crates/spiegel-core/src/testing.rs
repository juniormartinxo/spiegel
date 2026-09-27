//! Adb falso para testes: faz o papel do binário e do servidor adb, e os
//! testes controlam o que ele responde.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, mpsc};

use crate::adb::{AdbError, AdbLink, DeviceTracker};
use crate::device::Device;

pub struct FakeAdb {
    state: Mutex<State>,
    tracker_connected: Notify,
}

struct State {
    /// `None` simula o binário do adb ausente.
    client_version: Option<u32>,
    missing_path: PathBuf,
    server_version: Option<u32>,
    start_failure: Option<String>,
    tracker: Option<mpsc::Sender<Result<Vec<Device>, AdbError>>>,
    devices: Vec<Device>,
    starts: u32,
    kills: u32,
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
            state: Mutex::new(State {
                client_version,
                missing_path: PathBuf::new(),
                server_version: None,
                start_failure: None,
                tracker: None,
                devices: vec![],
                starts: 0,
                kills: 0,
            }),
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
            return Err(AdbError::Failed(detail));
        }
        state.server_version = state.client_version;
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
}
