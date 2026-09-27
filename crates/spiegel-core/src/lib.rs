//! Núcleo do Spiegel: acesso ao adb, registro de Dispositivos e Sessões com
//! o protocolo do scrcpy.
//!
//! O crate não depende do Tauri. A interface (ou um teste) usa só a API
//! pública daqui, que é a costura de teste descrita na spec (#4).

pub mod adb;
pub mod device;
pub mod registry;
pub mod session;
pub mod settings;

#[cfg(feature = "testing")]
pub mod testing;

pub use adb::{AdbError, AdbLink};
pub use device::{Device, DeviceState};
pub use registry::{AdbProblem, AdbStatus, DeviceRegistry, RegistryOptions, RegistrySnapshot, RestartHandle};
pub use session::{EndReason, Session, SessionEvent, SessionEvents, SessionOptions, StartupPhase, VideoCodec, VideoPacket};
pub use settings::Settings;
