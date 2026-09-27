//! Roda uma Sessão de Tela num Dispositivo real por alguns segundos e
//! resume os eventos no terminal, com o adb e o scrcpy-server embutidos.
//!
//! Uso: cargo run -p spiegel-core --example screen_session -- <serial> [segundos]
//! Precisa do adb embutido e do scrcpy-server (`pnpm fetch:adb`, `pnpm fetch:server`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use spiegel_core::adb::server_link::AdbServerLink;
use spiegel_core::{Session, SessionEvent, SessionOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let serial = args.next().ok_or("informe o serial")?;
    let seconds: u64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(3);

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources");
    let adb = root.join("platform-tools").join(if cfg!(windows) { "adb.exe" } else { "adb" });
    let link = Arc::new(AdbServerLink::new(adb));
    let started = tokio::time::Instant::now();
    let (session, mut events) = Session::start(link, serial, SessionOptions::new(root.join("scrcpy-server")));

    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let (mut packets, mut bytes) = (0usize, 0usize);
    let mut session = Some(session);
    loop {
        let event = tokio::select! {
            event = events.recv() => match event {
                Some(event) => event,
                None => break,
            },
            () = &mut deadline, if session.is_some() => {
                println!("{:>6} ms  parando", started.elapsed().as_millis());
                session.take().unwrap().stop().await;
                continue;
            }
        };
        match event {
            SessionEvent::VideoPacket(packet) => {
                if packets == 0 || packet.config || packet.key_frame {
                    println!(
                        "{:>6} ms  pacote config={} chave={} {} bytes",
                        started.elapsed().as_millis(),
                        packet.config,
                        packet.key_frame,
                        packet.data.len()
                    );
                }
                packets += 1;
                bytes += packet.data.len();
            }
            other => println!("{:>6} ms  {other:?}", started.elapsed().as_millis()),
        }
    }
    println!("{packets} pacotes, {bytes} bytes");
    Ok(())
}
