//! Testes na costura do núcleo: a Sessão de Tela vista de fora, como a
//! interface a usa, com o Dispositivo falso (e o `scrcpy-server` falso) no
//! lugar do real.

use std::path::PathBuf;
use std::time::Duration;

use spiegel_core::session::{
    EndReason, Session, SessionEvent, SessionEvents, SessionOptions, StartupPhase, VideoCodec, VideoPacket,
};
use spiegel_core::AdbProblem;
use spiegel_core::testing::{FakeAdb, FakeScrcpyServer, Push};

const SERIAL: &str = "2abaa661783f7ece";

fn options() -> SessionOptions {
    SessionOptions { server_path: PathBuf::from("resources/scrcpy-server"), connect_timeout: Duration::from_secs(2) }
}

async fn next(events: &mut SessionEvents) -> SessionEvent {
    tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("timed out waiting for a session event")
        .expect("event stream closed")
}

#[tokio::test]
async fn startup_pushes_the_server_opens_a_reverse_tunnel_and_starts_the_server() {
    let adb = FakeAdb::new(41).with_running_server(41);
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options());

    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::PushingServer });
    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::Connecting });
    assert_eq!(next(&mut events).await, SessionEvent::Connected { device_name: "SM-G9600".into() });

    assert_eq!(
        adb.pushes(),
        vec![Push {
            serial: SERIAL.into(),
            local: PathBuf::from("resources/scrcpy-server"),
            remote: "/data/local/tmp/scrcpy-server.jar".into(),
        }]
    );
    let shell = &adb.shell_commands()[0];
    assert_eq!(
        shell[..5],
        ["CLASSPATH=/data/local/tmp/scrcpy-server.jar", "app_process", "/", "com.genymobile.scrcpy.Server", "4.1"]
    );
    let scid = shell.iter().find_map(|arg| arg.strip_prefix("scid=")).expect("scid");
    assert_eq!(scid.len(), 8);
    assert!(u32::from_str_radix(scid, 16).unwrap() < 1 << 31, "scid has 31 bits");
    assert!(shell.contains(&"audio=false".to_owned()));
    assert!(!shell.iter().any(|arg| arg.starts_with("tunnel_forward")));
    // Como no scrcpy, o túnel sai logo depois que os sockets conectam.
    assert!(adb.open_tunnels().is_empty());
}

/// Consome os eventos da inicialização, até os sockets conectarem.
async fn skip_startup(events: &mut SessionEvents) {
    loop {
        match next(events).await {
            SessionEvent::Connected { .. } => return,
            SessionEvent::Phase { .. } => {}
            other => panic!("unexpected event during startup: {other:?}"),
        }
    }
}

fn packet(config: bool, key_frame: bool, pts: Option<u64>, data: &[u8]) -> SessionEvent {
    SessionEvent::VideoPacket(VideoPacket { config, key_frame, pts, data: data.to_vec() })
}

#[tokio::test]
async fn codec_size_and_packets_come_from_the_stream() {
    // Montado à mão a partir de doc/develop.md ("Video and audio") do scrcpy 4.1.
    let mut video = b"h264".to_vec();
    video.extend([0x80, 0, 0, 0, 0, 0, 0x02, 0xd0, 0, 0, 0x05, 0x00]); // sessão 720x1280
    video.extend([0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 1]); // config
    video.extend([0x20, 0, 0, 0, 0, 0, 0x01, 0x23, 0, 0, 0, 3, 0xaa, 0xbb, 0xcc]); // quadro-chave
    video.extend([0x00, 0, 0, 0, 0, 0, 0x01, 0x24, 0, 0, 0, 1, 0xdd]); // quadro
    video.extend([0x80, 0, 0, 0, 0, 0, 0x05, 0x00, 0, 0, 0x02, 0xd0]); // rotação: 1280x720
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.set_scrcpy_server(FakeScrcpyServer { video, ..Default::default() });
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options());
    skip_startup(&mut events).await;

    assert_eq!(next(&mut events).await, SessionEvent::VideoConfigured { codec: VideoCodec::H264, width: 720, height: 1280 });
    assert_eq!(next(&mut events).await, packet(true, false, None, &[0, 0, 0, 1]));
    assert_eq!(next(&mut events).await, packet(false, true, Some(0x123), &[0xaa, 0xbb, 0xcc]));
    assert_eq!(next(&mut events).await, packet(false, false, Some(0x124), &[0xdd]));
    assert_eq!(next(&mut events).await, SessionEvent::VideoConfigured { codec: VideoCodec::H264, width: 1280, height: 720 });
}

#[tokio::test]
async fn replays_a_real_stream_recorded_from_a_device() {
    let adb = FakeAdb::new(41).with_running_server(41);
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options());
    skip_startup(&mut events).await;

    assert_eq!(next(&mut events).await, SessionEvent::VideoConfigured { codec: VideoCodec::H264, width: 232, height: 480 });
    let mut packets = vec![];
    for _ in 0..12 {
        match next(&mut events).await {
            SessionEvent::VideoPacket(packet) => packets.push(packet),
            other => panic!("expected a video packet, got {other:?}"),
        }
    }
    // SPS + PPS no pacote de config, depois o quadro-chave e quadros a cada 100 ms.
    assert!(packets[0].config && packets[0].pts.is_none());
    assert_eq!(packets[0].data.len(), 29);
    assert_eq!(packets[0].data[..5], [0, 0, 0, 1, 0x67]);
    assert!(packets[1].key_frame);
    assert_eq!(packets[1].pts, Some(8_842_464_219));
    assert_eq!(packets[11].pts, Some(8_843_464_219));
    assert!(packets[2..].iter().all(|packet| !packet.config && !packet.key_frame));
}

async fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Pula os eventos até o fim da Sessão e devolve o motivo.
async fn end_reason(events: &mut SessionEvents) -> EndReason {
    loop {
        if let SessionEvent::Ended { reason } = next(events).await {
            assert!(events.recv().await.is_none(), "nothing comes after Ended");
            return reason;
        }
    }
}

#[tokio::test]
async fn stopping_closes_the_sockets_and_the_server_exits() {
    let adb = FakeAdb::new(41).with_running_server(41);
    let (session, mut events) = Session::start(adb.clone(), SERIAL, options());
    skip_startup(&mut events).await;
    assert_eq!(adb.running_servers(), 1);

    session.stop().await;

    assert_eq!(end_reason(&mut events).await, EndReason::Stopped);
    assert_eq!(adb.running_servers(), 0);
    assert!(adb.open_tunnels().is_empty());
}

#[tokio::test]
async fn stopping_during_startup_removes_the_tunnel() {
    let adb = FakeAdb::new(41).with_running_server(41);
    // Um servidor que nunca conecta: a Sessão fica esperando com o túnel aberto.
    adb.set_scrcpy_server(FakeScrcpyServer { never_connects: true, ..Default::default() });
    let (session, mut events) = Session::start(adb.clone(), SERIAL, options());
    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::PushingServer });
    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::Connecting });
    wait_until("the server starts", || adb.running_servers() == 1).await;
    assert_eq!(adb.open_tunnels().len(), 1);

    session.stop().await;

    assert_eq!(end_reason(&mut events).await, EndReason::Stopped);
    assert!(adb.open_tunnels().is_empty());
    assert_eq!(adb.running_servers(), 0);
}

#[tokio::test]
async fn a_closed_stream_ends_the_session_as_disconnected() {
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.set_scrcpy_server(FakeScrcpyServer { hang_up_after_video: true, ..Default::default() });
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options());

    assert_eq!(end_reason(&mut events).await, EndReason::Disconnected);
    assert_eq!(adb.running_servers(), 0);
    assert!(adb.open_tunnels().is_empty());
}

#[tokio::test]
async fn falls_back_to_a_forward_tunnel_when_reverse_fails() {
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.fail_reverse("more than one device/emulator");
    let (session, mut events) = Session::start(adb.clone(), SERIAL, options());

    skip_startup(&mut events).await;
    // O byte fictício do túnel direto não pode vazar para o fluxo de vídeo.
    assert_eq!(next(&mut events).await, SessionEvent::VideoConfigured { codec: VideoCodec::H264, width: 232, height: 480 });
    assert!(adb.shell_commands()[0].contains(&"tunnel_forward=true".to_owned()));
    assert!(adb.open_tunnels().is_empty());

    session.stop().await;
    assert_eq!(end_reason(&mut events).await, EndReason::Stopped);
    assert_eq!(adb.running_servers(), 0);
}

#[tokio::test]
async fn a_server_that_exits_before_connecting_ends_the_session_with_its_output() {
    let adb = FakeAdb::new(41).with_running_server(41);
    let output = "[server] ERROR: Could not create display (Android too old)";
    adb.set_scrcpy_server(FakeScrcpyServer { exit_before_connecting: Some(output.into()), ..Default::default() });
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options());

    assert_eq!(end_reason(&mut events).await, EndReason::ServerExited { output: output.into() });
    assert!(adb.open_tunnels().is_empty());
}

#[tokio::test]
async fn a_failed_push_ends_the_session_before_touching_the_tunnel() {
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.fail_push("device offline");
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options());

    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::PushingServer });
    assert_eq!(
        next(&mut events).await,
        SessionEvent::Ended { reason: EndReason::AdbFailed { problem: AdbProblem::CommandFailed { detail: "device offline".into() } } }
    );
    assert!(adb.shell_commands().is_empty());
}

#[tokio::test]
async fn a_server_that_never_connects_times_out() {
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.set_scrcpy_server(FakeScrcpyServer { never_connects: true, ..Default::default() });
    let options = SessionOptions { connect_timeout: Duration::from_millis(50), ..options() };
    let (_session, mut events) = Session::start(adb.clone(), SERIAL, options);

    assert_eq!(end_reason(&mut events).await, EndReason::ConnectTimeout);
    assert!(adb.open_tunnels().is_empty());
    assert_eq!(adb.running_servers(), 0);
}

async fn end_reason_for_stream(video: Vec<u8>) -> EndReason {
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.set_scrcpy_server(FakeScrcpyServer { video, ..Default::default() });
    let (_session, mut events) = Session::start(adb, SERIAL, options());
    end_reason(&mut events).await
}

#[tokio::test]
async fn a_device_that_disables_the_video_says_so() {
    // Codec id 0: o Dispositivo desligou o fluxo (demuxer.c do scrcpy 4.1).
    assert_eq!(end_reason_for_stream(vec![0, 0, 0, 0]).await, EndReason::VideoDisabled);
}

#[tokio::test]
async fn a_device_that_cannot_configure_the_encoder_says_so() {
    // Codec id 1: erro de configuração do fluxo no Dispositivo.
    assert_eq!(end_reason_for_stream(vec![0, 0, 0, 1]).await, EndReason::VideoConfigFailed);
}

#[tokio::test]
async fn an_empty_packet_is_a_protocol_error() {
    let mut video = b"h264".to_vec();
    video.extend([0x80, 0, 0, 0, 0, 0, 0x02, 0xd0, 0, 0, 0x05, 0x00]);
    video.extend([0x00, 0, 0, 0, 0, 0, 0x01, 0x24, 0, 0, 0, 0]);
    assert!(matches!(end_reason_for_stream(video).await, EndReason::ProtocolError { .. }));
}

#[tokio::test]
async fn stopping_during_the_forward_command_still_removes_the_tunnel() {
    let adb = FakeAdb::new(41).with_running_server(41);
    adb.fail_reverse("more than one device/emulator");
    adb.delay_forward(Duration::from_millis(200));
    let (session, mut events) = Session::start(adb.clone(), SERIAL, options());
    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::PushingServer });
    assert_eq!(next(&mut events).await, SessionEvent::Phase { phase: StartupPhase::Connecting });
    // O adb já criou o túnel, mas ainda não respondeu.
    wait_until("the forward command starts", || adb.open_tunnels().len() == 1).await;

    session.stop().await;

    assert_eq!(end_reason(&mut events).await, EndReason::Stopped);
    // O `adb forward` termina depois da parada, e o túnel dele é desfeito.
    wait_until("the late forward tunnel is removed", || adb.open_tunnels().is_empty()).await;
}
