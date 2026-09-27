//! Versão real do [`AdbLink`]: fala com o servidor adb pelo socket (porta
//! 5037) e usa o binário do adb só para descobrir a própria versão e iniciar
//! o servidor.

use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;

use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::mpsc;

use super::protocol::{parse_hex, read_block, read_status, send_request};
use super::{AdbError, AdbLink, DeviceTracker};
use crate::device::parse_device_list;

pub const DEFAULT_SERVER_ADDR: SocketAddr = SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::LOCALHOST,
    5037,
));

pub struct AdbServerLink {
    adb_path: PathBuf,
    server_addr: SocketAddr,
}

impl AdbServerLink {
    pub fn new(adb_path: PathBuf) -> Self {
        Self::with_server_addr(adb_path, DEFAULT_SERVER_ADDR)
    }

    pub fn with_server_addr(adb_path: PathBuf, server_addr: SocketAddr) -> Self {
        Self { adb_path, server_addr }
    }

    fn command(&self) -> Result<Command, AdbError> {
        if !self.adb_path.is_file() {
            return Err(AdbError::NotFound(self.adb_path.clone()));
        }
        let mut command = Command::new(&self.adb_path);
        command.stdin(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: não pisca um console a cada chamada do adb.
            command.creation_flags(0x0800_0000);
        }
        Ok(command)
    }

    /// Abre uma conexão e envia um serviço "host". `None` se não houver
    /// servidor escutando.
    async fn host_request(&self, service: &str) -> Result<Option<TcpStream>, AdbError> {
        let mut stream = match TcpStream::connect(self.server_addr).await {
            Ok(stream) => stream,
            Err(err) if err.kind() == ErrorKind::ConnectionRefused => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        send_request(&mut stream, service).await?;
        read_status(&mut stream).await?;
        Ok(Some(stream))
    }
}

impl AdbLink for AdbServerLink {
    async fn client_version(&self) -> Result<u32, AdbError> {
        let output = self
            .command()?
            .arg("version")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .map_err(AdbError::Spawn)?;
        let text = String::from_utf8_lossy(&output.stdout);
        parse_client_version(&text)
            .ok_or_else(|| AdbError::Failed(format!("saída inesperada de `adb version`: {}", text.trim())))
    }

    async fn server_version(&self) -> Result<Option<u32>, AdbError> {
        let Some(mut stream) = self.host_request("host:version").await? else {
            return Ok(None);
        };
        let payload = read_block(&mut stream)
            .await?
            .ok_or_else(|| AdbError::Protocol("host:version sem resposta".into()))?;
        parse_hex(payload.as_bytes()).map(Some)
    }

    async fn start_server(&self) -> Result<(), AdbError> {
        // Tudo em Stdio::null(): o servidor adb que nasce daqui herda os
        // handles, e segurar um pipe aberto travaria a espera.
        let status = self
            .command()?
            .arg("start-server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false)
            .status()
            .await
            .map_err(AdbError::Spawn)?;
        if status.success() {
            Ok(())
        } else {
            Err(AdbError::Failed(format!("`adb start-server` terminou com {status}")))
        }
    }

    async fn kill_server(&self) -> Result<(), AdbError> {
        match self.host_request("host:kill").await {
            Ok(_) => Ok(()),
            // O servidor pode fechar a conexão antes de responder.
            Err(AdbError::Io(err)) if err.kind() == ErrorKind::UnexpectedEof => Ok(()),
            Err(err) => Err(err),
        }
    }

    async fn track_devices(&self) -> Result<DeviceTracker, AdbError> {
        let mut stream = self
            .host_request("host:track-devices-l")
            .await?
            .ok_or_else(|| AdbError::Io(ErrorKind::ConnectionRefused.into()))?;
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            loop {
                let item = match read_block(&mut stream).await {
                    Ok(Some(text)) => Ok(parse_device_list(&text)),
                    Ok(None) => break,
                    Err(err) => Err(err),
                };
                let failed = item.is_err();
                if tx.send(item).await.is_err() || failed {
                    break;
                }
            }
        });
        Ok(rx)
    }
}

/// "Android Debug Bridge version 1.0.41" → 41.
fn parse_client_version(text: &str) -> Option<u32> {
    let line = text.lines().find(|line| line.starts_with("Android Debug Bridge version "))?;
    let version = line.rsplit(' ').next()?;
    version.rsplit('.').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_client_version;

    #[test]
    fn versao_do_cliente() {
        let text = "Android Debug Bridge version 1.0.41\nVersion 36.0.0-13206524\nInstalled as C:\\adb.exe\n";
        assert_eq!(parse_client_version(text), Some(41));
        assert_eq!(parse_client_version("lixo"), None);
    }
}
