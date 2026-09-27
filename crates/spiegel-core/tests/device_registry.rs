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
    found.unwrap_or_else(|| panic!("tempo esgotado esperando: {what}; último estado: {:?}", *rx.borrow()))
}

#[tokio::test]
async fn inicia_o_servidor_e_mostra_o_dispositivo_plugado() {
    let adb = FakeAdb::new(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    wait_for(&mut rx, "adb pronto", |s| s.adb == AdbStatus::Ready { version: 41 }).await;
    assert_eq!(adb.starts(), 1);

    let pixel = device("R58M123", Some("Pixel 7"), DeviceState::Ready);
    adb.set_devices(vec![pixel.clone()]).await;
    let snapshot = wait_for(&mut rx, "Dispositivo na lista", |s| !s.devices.is_empty()).await;
    assert_eq!(snapshot.devices, vec![pixel]);
}

#[tokio::test]
async fn acompanha_mudanca_de_estado_e_remocao() {
    let adb = FakeAdb::new(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    adb.set_devices(vec![device("abc", None, DeviceState::Unauthorized)]).await;
    wait_for(&mut rx, "não autorizado", |s| s.devices.iter().any(|d| d.state == DeviceState::Unauthorized)).await;

    adb.set_devices(vec![device("abc", Some("Pixel 7"), DeviceState::Ready)]).await;
    let snapshot = wait_for(&mut rx, "autorizado", |s| s.devices.iter().any(|d| d.state == DeviceState::Ready)).await;
    assert_eq!(snapshot.devices[0].model.as_deref(), Some("Pixel 7"));

    adb.set_devices(vec![]).await;
    wait_for(&mut rx, "lista vazia", |s| s.devices.is_empty()).await;
}

#[tokio::test]
async fn usa_um_servidor_da_mesma_versao_que_ja_esta_rodando() {
    let adb = FakeAdb::new(41).with_running_server(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    wait_for(&mut rx, "adb pronto", |s| s.adb == AdbStatus::Ready { version: 41 }).await;
    assert_eq!(adb.starts(), 0);
    assert_eq!(adb.kills(), 0);
}

#[tokio::test]
async fn servidor_de_outra_versao_e_informado_e_nao_derrubado() {
    let adb = FakeAdb::new(41).with_running_server(40);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    let conflict = AdbStatus::Conflict { server_version: 40, client_version: 41 };
    wait_for(&mut rx, "conflito", |s| s.adb == conflict).await;

    // Mesmo com o conflito, os Dispositivos continuam aparecendo.
    adb.set_devices(vec![device("abc", Some("Pixel 7"), DeviceState::Ready)]).await;
    wait_for(&mut rx, "Dispositivo durante o conflito", |s| s.adb == conflict && s.devices.len() == 1).await;
    assert_eq!(adb.kills(), 0, "o servidor do usuário não pode ser derrubado sem ele pedir");

    // O usuário escolhe reiniciar com o adb do Spiegel.
    registry.restart_server().await;
    wait_for(&mut rx, "adb do Spiegel pronto", |s| s.adb == AdbStatus::Ready { version: 41 }).await;
    assert_eq!(adb.kills(), 1);
    assert_eq!(adb.starts(), 1);
}

#[tokio::test]
async fn binario_ausente_fica_indisponivel() {
    let adb = FakeAdb::missing("C:/nao/existe/adb.exe");
    let registry = DeviceRegistry::start(adb, options());
    let mut rx = registry.subscribe();

    let snapshot = wait_for(&mut rx, "indisponível", |s| matches!(s.adb, AdbStatus::Unavailable { .. })).await;
    assert_eq!(
        snapshot.adb,
        AdbStatus::Unavailable { problem: AdbProblem::NotFound { path: "C:/nao/existe/adb.exe".into() } }
    );
}

#[tokio::test]
async fn falha_ao_iniciar_o_servidor_e_informada() {
    let adb = FakeAdb::new(41);
    adb.fail_start("porta 5037 ocupada");
    let registry = DeviceRegistry::start(adb, options());
    let mut rx = registry.subscribe();

    let snapshot = wait_for(&mut rx, "indisponível", |s| matches!(s.adb, AdbStatus::Unavailable { .. })).await;
    let AdbStatus::Unavailable { problem: AdbProblem::Failed { detail } } = snapshot.adb else {
        panic!("esperava falha ao iniciar, veio {:?}", snapshot.adb);
    };
    assert!(detail.contains("porta 5037 ocupada"), "{detail}");
}

#[tokio::test]
async fn servidor_que_morre_limpa_a_lista_e_volta_sozinho() {
    let adb = FakeAdb::new(41);
    let registry = DeviceRegistry::start(adb.clone(), options());
    let mut rx = registry.subscribe();

    let pixel = device("abc", Some("Pixel 7"), DeviceState::Ready);
    adb.set_devices(vec![pixel.clone()]).await;
    wait_for(&mut rx, "Dispositivo na lista", |s| s.devices.len() == 1).await;

    adb.kill_externally();
    wait_for(&mut rx, "lista limpa", |s| s.devices.is_empty()).await;

    // O registro inicia o servidor de novo e a lista atual volta.
    let snapshot =
        wait_for(&mut rx, "de volta", |s| s.adb == AdbStatus::Ready { version: 41 } && s.devices.len() == 1).await;
    assert_eq!(snapshot.devices, vec![pixel]);
    assert_eq!(adb.starts(), 2);
}
