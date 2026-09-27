//! Mostra no terminal o estado do adb e os Dispositivos, ao vivo, usando o
//! adb real. Útil para testar o núcleo sem abrir o aplicativo.
//!
//! cargo run -p spiegel-core --example watch_devices -- <caminho do adb>

use std::path::PathBuf;
use std::sync::Arc;

use spiegel_core::adb::server_link::AdbServerLink;
use spiegel_core::{DeviceRegistry, RegistryOptions};

#[tokio::main]
async fn main() {
    let adb = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../src-tauri/resources/platform-tools/adb.exe"))
    });
    println!("usando {}", adb.display());
    let registry = DeviceRegistry::start(Arc::new(AdbServerLink::new(adb)), RegistryOptions::default());
    let mut changes = registry.subscribe();
    loop {
        println!("{:?}", *changes.borrow_and_update());
        if changes.changed().await.is_err() {
            break;
        }
    }
}
