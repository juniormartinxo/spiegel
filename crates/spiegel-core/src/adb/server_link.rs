//! Versão real do [`AdbLink`]: fala com o servidor adb pelo socket (porta
//! 5037) e usa o binário do adb só para descobrir a própria versão e iniciar
//! o servidor.

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;
use tokio::process::{ChildStderr, ChildStdout, Command};
use tokio::sync::mpsc;

use super::protocol::{parse_hex, read_block, read_status, send_request};
use super::{AdbError, AdbLink, DeviceProcess, DeviceTracker};
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

    /// Um comando do adb para um Dispositivo, no mesmo servidor que o link usa.
    fn device_command(&self, serial: &str) -> Result<Command, AdbError> {
        let mut command = self.command()?;
        command.arg("-P").arg(self.server_addr.port().to_string()).arg("-s").arg(serial);
        Ok(command)
    }

    /// Roda um comando curto do adb e devolve o stdout, ou o erro com o stderr.
    async fn run_device(&self, serial: &str, args: &[&OsStr]) -> Result<String, AdbError> {
        let output = self
            .device_command(serial)?
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(AdbError::Spawn)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let command = args.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>().join(" ");
            Err(AdbError::CommandFailed(format!("adb {command}: {}", stderr.trim())))
        }
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
            .ok_or_else(|| AdbError::UnexpectedOutput(format!("adb version: {}", text.trim())))
    }

    async fn server_version(&self) -> Result<Option<u32>, AdbError> {
        let Some(mut stream) = self.host_request("host:version").await? else {
            return Ok(None);
        };
        let payload = read_block(&mut stream)
            .await?
            .ok_or_else(|| AdbError::Protocol("host:version without a reply".into()))?;
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
            Err(AdbError::CommandFailed(format!("adb start-server: {status}")))
        }
    }

    async fn kill_server(&self) -> Result<(), AdbError> {
        match self.host_request("host:kill").await {
            // Lê até o servidor fechar a conexão, sinal de que está saindo.
            Ok(Some(mut stream)) => {
                let mut rest = Vec::new();
                let _ = stream.read_to_end(&mut rest).await;
            }
            Ok(None) => return Ok(()),
            // O servidor pode fechar a conexão antes de responder; no
            // Windows isso chega como reset ou abort, não só como EOF.
            Err(AdbError::Io(err)) if closed_abruptly(&err) => {}
            Err(err) => return Err(err),
        }
        self.wait_until_port_is_free().await
    }

    async fn track_devices(&self) -> Result<DeviceTracker, AdbError> {
        let mut stream = self
            .host_request("host:track-devices-l")
            .await?
            .ok_or_else(|| AdbError::Io(ErrorKind::ConnectionRefused.into()))?;
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            loop {
                // Termina também quando ninguém mais escuta, para não segurar
                // o socket até o próximo bloco chegar.
                let block = tokio::select! {
                    block = read_block(&mut stream) => block,
                    () = tx.closed() => break,
                };
                let item = match block {
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

    async fn push(&self, serial: &str, local: &Path, remote: &str) -> Result<(), AdbError> {
        self.run_device(serial, &["push".as_ref(), local.as_os_str(), remote.as_ref()]).await.map(drop)
    }

    async fn reverse(&self, serial: &str, device_socket: &str, local_port: u16) -> Result<(), AdbError> {
        let local = format!("tcp:{local_port}");
        self.run_device(serial, &["reverse".as_ref(), device_socket.as_ref(), local.as_ref()]).await.map(drop)
    }

    async fn remove_reverse(&self, serial: &str, device_socket: &str) -> Result<(), AdbError> {
        self.run_device(serial, &["reverse".as_ref(), "--remove".as_ref(), device_socket.as_ref()]).await.map(drop)
    }

    async fn forward(&self, serial: &str, device_socket: &str) -> Result<u16, AdbError> {
        // Com `tcp:0`, o adb escolhe uma porta livre e a imprime.
        let stdout = self.run_device(serial, &["forward".as_ref(), "tcp:0".as_ref(), device_socket.as_ref()]).await?;
        stdout
            .trim()
            .parse()
            .map_err(|_| AdbError::UnexpectedOutput(format!("adb forward: {}", stdout.trim())))
    }

    async fn remove_forward(&self, serial: &str, local_port: u16) -> Result<(), AdbError> {
        let local = format!("tcp:{local_port}");
        self.run_device(serial, &["forward".as_ref(), "--remove".as_ref(), local.as_ref()]).await.map(drop)
    }

    async fn spawn_shell(&self, serial: &str, args: &[String]) -> Result<DeviceProcess, AdbError> {
        let mut child = self
            .device_command(serial)?
            .arg("shell")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(AdbError::Spawn)?;
        let (process, mut control) = DeviceProcess::new();
        let output = tokio::spawn(collect_output(child.stdout.take(), child.stderr.take()));
        tokio::spawn(async move {
            tokio::select! {
                _ = child.wait() => {}
                () = control.kill_requested() => {
                    let _ = child.kill().await;
                }
            }
            // Um processo herdeiro dos pipes (ex.: um servidor adb iniciado
            // pelo comando) poderia segurar a saída aberta para sempre.
            let output = tokio::time::timeout(OUTPUT_GRACE, output).await;
            control.exited(output.ok().and_then(Result::ok).unwrap_or_default());
        });
        Ok(process)
    }
}

/// Junta a saída do processo, até um limite, para diagnosticar falhas.
async fn collect_output(stdout: Option<ChildStdout>, stderr: Option<ChildStderr>) -> String {
    async fn read_capped(reader: Option<impl AsyncRead + Unpin>) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some(reader) = reader {
            let _ = reader.take(OUTPUT_LIMIT).read_to_end(&mut bytes).await;
        }
        bytes
    }
    let (out, err) = tokio::join!(read_capped(stdout), read_capped(stderr));
    let mut text = String::from_utf8_lossy(&out).into_owned();
    text.push_str(&String::from_utf8_lossy(&err));
    text
}

const OUTPUT_LIMIT: u64 = 16 * 1024;
const OUTPUT_GRACE: Duration = Duration::from_secs(1);

impl AdbServerLink {
    /// Espera o servidor antigo largar a porta. Sem isso, a próxima conexão
    /// ainda pode cair no servidor que está morrendo, ou o `start-server`
    /// falhar com a porta ocupada.
    async fn wait_until_port_is_free(&self) -> Result<(), AdbError> {
        let deadline = tokio::time::Instant::now() + KILL_TIMEOUT;
        loop {
            match TcpStream::connect(self.server_addr).await {
                Err(err) if err.kind() == ErrorKind::ConnectionRefused => return Ok(()),
                Err(err) => return Err(err.into()),
                Ok(_) if tokio::time::Instant::now() >= deadline => {
                    return Err(AdbError::Timeout("adb server did not exit after host:kill"));
                }
                Ok(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }
}

const KILL_TIMEOUT: Duration = Duration::from_secs(5);

fn closed_abruptly(err: &std::io::Error) -> bool {
    matches!(err.kind(), ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted)
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
    fn client_version() {
        let text = "Android Debug Bridge version 1.0.41\nVersion 36.0.0-13206524\nInstalled as C:\\adb.exe\n";
        assert_eq!(parse_client_version(text), Some(41));
        assert_eq!(parse_client_version("lixo"), None);
    }
}
