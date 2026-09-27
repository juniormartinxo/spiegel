//! Testes na costura do núcleo: o registro de Dispositivos visto de fora,
//! como a interface o usa, com o adb falso no lugar do real.

use std::time::Duration;

use spiegel_core::testing::FakeAdb;
use spiegel_core::{AdbProblem, AdbStatus, Device, DeviceRegistry, DeviceState, RegistryOptions, RegistrySnapshot};
use tokio::sync::watch;

fn options() -> RegistryOptions {
    RegistryOptions { retry_delay: Duration::from_millis(10) }
}

fn device(serial: &str, model: Option<&str>, state: DeviceState) -> Device {
    Device { serial: serial.into(), model: model.map(Into::into), state }
}

async fn wait_for(
    rx: &mut watch::Receiver<RegistrySnapshot>,
    what: &str,
    predicate: impl Fn(&RegistrySnapshot) -> bool,
) -> RegistrySnapshot {
    let found = tokio::time::timeout(Duration::from_secs(2), rx.wait_for(|snapshot| predicate(snapshot)))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|snapshot| snapshot.clone());
    found.unwrap_or_else(|| panic!("timed out waiting for: {what}; last state: {:?}", *rx.borrow()))
}

#[tokio::test]
async fn starts_the_server_and_lists_a_plugged_device() {
    let adb = FakeAdb::new(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    wait_for(&mut rx, "adb ready", |s| s.adb == AdbStatus::Ready { version: 41 }).await;
    assert_eq!(adb.starts(), 1);

    let pixel = device("R58M123", Some("Pixel 7"), DeviceState::Ready);
    adb.set_devices(vec![pixel.clone()]).await;
    let snapshot = wait_for(&mut rx, "device listed", |s| !s.devices.is_empty()).await;
    assert_eq!(snapshot.devices, vec![pixel]);
}

#[tokio::test]
async fn follows_state_changes_and_removal() {
    let adb = FakeAdb::new(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    adb.set_devices(vec![device("abc", None, DeviceState::Unauthorized)]).await;
    wait_for(&mut rx, "unauthorized", |s| s.devices.iter().any(|d| d.state == DeviceState::Unauthorized)).await;

    adb.set_devices(vec![device("abc", Some("Pixel 7"), DeviceState::Ready)]).await;
    let snapshot = wait_for(&mut rx, "authorized", |s| s.devices.iter().any(|d| d.state == DeviceState::Ready)).await;
    assert_eq!(snapshot.devices[0].model.as_deref(), Some("Pixel 7"));

    adb.set_devices(vec![]).await;
    wait_for(&mut rx, "empty list", |s| s.devices.is_empty()).await;
}

#[tokio::test]
async fn reuses_a_running_server_of_the_same_version() {
    let adb = FakeAdb::new(41).with_running_server(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    wait_for(&mut rx, "adb ready", |s| s.adb == AdbStatus::Ready { version: 41 }).await;
    assert_eq!(adb.starts(), 0);
    assert_eq!(adb.kills(), 0);
}

#[tokio::test]
async fn reports_a_server_of_another_version_without_killing_it() {
    let adb = FakeAdb::new(41).with_running_server(40);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    let conflict = AdbStatus::Conflict { server_version: 40, client_version: 41 };
    wait_for(&mut rx, "conflict", |s| s.adb == conflict).await;

    // Mesmo com o conflito, os Dispositivos continuam aparecendo.
    adb.set_devices(vec![device("abc", Some("Pixel 7"), DeviceState::Ready)]).await;
    wait_for(&mut rx, "device during the conflict", |s| s.adb == conflict && s.devices.len() == 1).await;
    assert_eq!(adb.kills(), 0, "the user's server must not be killed unless they ask");
}

#[tokio::test]
async fn restart_requested_by_the_user_finishes_with_the_new_state_published() {
    let adb = FakeAdb::new(41).with_running_server(40);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();
    wait_for(&mut rx, "conflict", |s| matches!(s.adb, AdbStatus::Conflict { .. })).await;

    // O pedido só retorna depois do reinício, com o estado novo já publicado.
    registry.restart_handle().restart_server().await;
    assert_eq!(registry.snapshot().adb, AdbStatus::Ready { version: 41 });
    assert_eq!(adb.kills(), 1);
    assert_eq!(adb.starts(), 1);
}

#[tokio::test]
async fn a_foreign_server_started_during_startup_is_a_conflict() {
    let adb = FakeAdb::new(41);
    adb.foreign_server_appears_on_start(39);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    wait_for(&mut rx, "conflict", |s| s.adb == AdbStatus::Conflict { server_version: 39, client_version: 41 }).await;
    assert_eq!(adb.kills(), 0);
}

#[tokio::test]
async fn missing_binary_is_unavailable() {
    let adb = FakeAdb::missing("C:/does/not/exist/adb.exe");
    let registry = DeviceRegistry::start(adb, options());
    let mut rx = registry.subscribe();

    let snapshot = wait_for(&mut rx, "unavailable", |s| matches!(s.adb, AdbStatus::Unavailable { .. })).await;
    assert_eq!(
        snapshot.adb,
        AdbStatus::Unavailable { problem: AdbProblem::NotFound { path: "C:/does/not/exist/adb.exe".into() } }
    );
}

#[tokio::test]
async fn failing_to_start_the_server_is_reported() {
    let adb = FakeAdb::new(41);
    adb.fail_start("port 5037 in use");
    let registry = DeviceRegistry::start(adb, options());
    let mut rx = registry.subscribe();

    let snapshot = wait_for(&mut rx, "unavailable", |s| matches!(s.adb, AdbStatus::Unavailable { .. })).await;
    assert_eq!(
        snapshot.adb,
        AdbStatus::Unavailable { problem: AdbProblem::CommandFailed { detail: "port 5037 in use".into() } }
    );
}

#[tokio::test]
async fn a_dying_server_clears_the_list_and_comes_back_by_itself() {
    let adb = FakeAdb::new(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    let pixel = device("abc", Some("Pixel 7"), DeviceState::Ready);
    adb.set_devices(vec![pixel.clone()]).await;
    wait_for(&mut rx, "device listed", |s| s.devices.len() == 1).await;

    adb.kill_externally();
    wait_for(&mut rx, "list cleared", |s| s.devices.is_empty()).await;

    // O registro inicia o servidor de novo e a lista atual volta.
    let snapshot =
        wait_for(&mut rx, "back", |s| s.adb == AdbStatus::Ready { version: 41 } && s.devices.len() == 1).await;
    assert_eq!(snapshot.devices, vec![pixel]);
    assert_eq!(adb.starts(), 2);
}
