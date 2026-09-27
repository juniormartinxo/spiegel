//! Grava alguns segundos do fluxo de vídeo cru de um Dispositivo real, do
//! jeito que o `scrcpy-server` 4.1 o envia (id do codec, pacotes de sessão e
//! pacotes de mídia, sem os metadados do Dispositivo). É assim que a fixture
//! do Dispositivo falso (`fixtures/screen-h264.bin`) foi gerada.
//!
//! Uso: cargo run -p spiegel-core --example record_screen -- <serial> <saída> [segundos]
//! Precisa do adb embutido e do scrcpy-server (`pnpm fetch:adb`, `pnpm fetch:server`).

use std::path::PathBuf;
use std::time::Duration;

use spiegel_core::adb::server_link::AdbServerLink;
use spiegel_core::session::{DEVICE_SERVER_PATH, SCRCPY_VERSION};
use spiegel_core::AdbLink;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let serial = args.next().ok_or("informe o serial")?;
    let output = PathBuf::from(args.next().ok_or("informe o arquivo de saída")?);
    let seconds: u64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(3);

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources");
    let adb = root.join("platform-tools").join(if cfg!(windows) { "adb.exe" } else { "adb" });
    let link = AdbServerLink::new(adb);

    link.push(&serial, &root.join("scrcpy-server"), DEVICE_SERVER_PATH).await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let socket = "localabstract:scrcpy_00c0ffee";
    link.reverse(&serial, socket, port).await?;

    // Tamanho e taxa de bits baixos para a fixture ficar pequena.
    let server_args: Vec<String> = [
        &format!("CLASSPATH={DEVICE_SERVER_PATH}"),
        "app_process",
        "/",
        "com.genymobile.scrcpy.Server",
        SCRCPY_VERSION,
        "scid=00c0ffee",
        "log_level=info",
        "audio=false",
        "control=false",
        "max_size=480",
        "video_bit_rate=500000",
        "max_fps=15",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let mut process = link.spawn_shell(&serial, &server_args).await?;

    let (mut video, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept()).await??;
    link.remove_reverse(&serial, socket).await?;

    let mut name = [0u8; 64];
    video.read_exact(&mut name).await?;
    let name = String::from_utf8_lossy(&name);
    println!("Dispositivo: {}", name.trim_end_matches('\0'));

    let mut stream = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(seconds), video.read_to_end(&mut stream)).await;
    drop(video);
    println!("{} bytes recebidos", stream.len());
    stream.truncate(complete_packets_len(&stream));
    std::fs::write(&output, &stream)?;
    println!("{} bytes em {}", stream.len(), output.display());

    process.kill();
    let exit = process.exited().await;
    println!("saída do servidor:\n{}", exit.output);
    Ok(())
}

/// Até onde vão os pacotes completos: a gravação para no meio de um pacote,
/// e a fixture deve terminar entre dois.
fn complete_packets_len(stream: &[u8]) -> usize {
    let mut end = 4; // id do codec
    while stream.len() >= end + 12 {
        let header = &stream[end..end + 12];
        let size = if header[0] & 0x80 != 0 {
            0 // pacote de sessão: só o cabeçalho
        } else {
            u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize
        };
        if stream.len() < end + 12 + size {
            break;
        }
        end += 12 + size;
    }
    end.min(stream.len())
}
