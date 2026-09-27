//! A versão real do `AdbLink` contra um servidor adb falso na rede local,
//! para conferir o protocolo "host" de ponta a ponta.

use std::path::PathBuf;

use spiegel_core::adb::server_link::AdbServerLink;
use spiegel_core::{AdbError, AdbLink, DeviceState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Aceita uma conexão, confere o serviço pedido e responde com `reply`.
async fn fake_server(expected_service: &'static str, reply: Vec<u8>) -> (AdbServerLink, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut prefix = [0u8; 4];
        socket.read_exact(&mut prefix).await.unwrap();
        let len = usize::from_str_radix(std::str::from_utf8(&prefix).unwrap(), 16).unwrap();
        let mut service = vec![0u8; len];
        socket.read_exact(&mut service).await.unwrap();
        assert_eq!(std::str::from_utf8(&service).unwrap(), expected_service);
        socket.write_all(&reply).await.unwrap();
    });
    (AdbServerLink::with_server_addr(PathBuf::from("adb-unused"), addr), task)
}

fn block(text: &str) -> Vec<u8> {
    format!("{:04x}{text}", text.len()).into_bytes()
}

#[tokio::test]
async fn reads_the_server_version() {
    let mut reply = b"OKAY".to_vec();
    reply.extend(block("0029"));
    let (link, task) = fake_server("host:version", reply).await;

    assert_eq!(link.server_version().await.unwrap(), Some(41));
    task.await.unwrap();
}

#[tokio::test]
async fn no_server_means_no_version() {
    // Uma porta que acabou de ser liberada: ninguém escuta nela.
    let addr = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let link = AdbServerLink::with_server_addr(PathBuf::from("adb-unused"), addr);

    assert_eq!(link.server_version().await.unwrap(), None);
}

#[tokio::test]
async fn tracks_device_lists() {
    let mut reply = b"OKAY".to_vec();
    reply.extend(block(""));
    reply.extend(block("R58M123    unauthorized transport_id:1\n"));
    reply.extend(block("R58M123    device usb:1-4 product:p model:Pixel_7 device:d transport_id:1\n"));
    let (link, task) = fake_server("host:track-devices-l", reply).await;

    let mut tracker = link.track_devices().await.unwrap();
    assert!(tracker.recv().await.unwrap().unwrap().is_empty());
    let unauthorized = tracker.recv().await.unwrap().unwrap();
    assert_eq!(unauthorized[0].state, DeviceState::Unauthorized);
    let ready = tracker.recv().await.unwrap().unwrap();
    assert_eq!((ready[0].state.clone(), ready[0].model.as_deref()), (DeviceState::Ready, Some("Pixel 7")));

    task.await.unwrap();
    // O servidor fechou: o acompanhamento termina.
    assert!(tracker.recv().await.is_none());
}

#[tokio::test]
async fn a_server_rejection_is_an_error() {
    let mut reply = b"FAIL".to_vec();
    reply.extend(block("unknown host service"));
    let (link, task) = fake_server("host:track-devices-l", reply).await;

    let err = link.track_devices().await.unwrap_err();
    assert!(matches!(err, AdbError::Rejected(msg) if msg == "unknown host service"));
    task.await.unwrap();
}

#[tokio::test]
async fn kill_waits_for_the_server_to_leave_the_port() {
    // O servidor responde OKAY ao host:kill e sai: fecha a conexão e larga a porta.
    let (link, task) = fake_server("host:kill", b"OKAY".to_vec()).await;

    link.kill_server().await.unwrap();
    task.await.unwrap();
    assert_eq!(link.server_version().await.unwrap(), None);
}

#[tokio::test]
async fn a_missing_binary_is_not_found() {
    let link = AdbServerLink::new(PathBuf::from("C:/does/not/exist/adb.exe"));
    assert!(matches!(link.client_version().await, Err(AdbError::NotFound(_))));
}
