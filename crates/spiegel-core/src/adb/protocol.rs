//! Enquadramento do protocolo "host" do servidor adb (a porta 5037).
//!
//! Cada pedido é o comprimento em 4 dígitos hexadecimais seguido do nome do
//! serviço (ex.: `000Chost:version`). O servidor responde `OKAY` ou `FAIL` +
//! mensagem, e os dados vêm em blocos com o mesmo prefixo de comprimento.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::AdbError;

pub async fn send_request<W: AsyncWrite + Unpin>(writer: &mut W, service: &str) -> Result<(), AdbError> {
    let request = format!("{:04x}{service}", service.len());
    writer.write_all(request.as_bytes()).await?;
    Ok(())
}

/// Lê `OKAY`, ou transforma `FAIL` + mensagem em [`AdbError::Rejected`].
pub async fn read_status<R: AsyncRead + Unpin>(reader: &mut R) -> Result<(), AdbError> {
    let mut status = [0u8; 4];
    reader.read_exact(&mut status).await?;
    match &status {
        b"OKAY" => Ok(()),
        b"FAIL" => {
            let message = read_block(reader).await?.unwrap_or_default();
            Err(AdbError::Rejected(message))
        }
        other => Err(AdbError::Protocol(format!("unexpected status {:?}", String::from_utf8_lossy(other)))),
    }
}

/// Lê um bloco com prefixo de comprimento. Devolve `None` se a conexão
/// fechar exatamente entre dois blocos.
pub async fn read_block<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<String>, AdbError> {
    let mut prefix = [0u8; 4];
    let mut filled = 0;
    while filled < prefix.len() {
        let n = reader.read(&mut prefix[filled..]).await?;
        if n == 0 {
            return if filled == 0 {
                Ok(None)
            } else {
                Err(AdbError::Protocol("connection closed inside a length prefix".into()))
            };
        }
        filled += n;
    }
    let len = parse_hex(&prefix)? as usize;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    String::from_utf8(payload)
        .map(Some)
        .map_err(|_| AdbError::Protocol("block is not UTF-8".into()))
}

pub fn parse_hex(digits: &[u8]) -> Result<u32, AdbError> {
    std::str::from_utf8(digits)
        .ok()
        .and_then(|text| u32::from_str_radix(text, 16).ok())
        .ok_or_else(|| AdbError::Protocol(format!("invalid hex {:?}", String::from_utf8_lossy(digits))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn request_has_a_hex_length_prefix() {
        let mut out = Vec::new();
        send_request(&mut out, "host:version").await.unwrap();
        assert_eq!(out, b"000chost:version");
    }

    #[tokio::test]
    async fn fail_becomes_rejected_with_the_message() {
        let mut input: &[u8] = b"FAIL0007unknown";
        let err = read_status(&mut input).await.unwrap_err();
        assert!(matches!(err, AdbError::Rejected(msg) if msg == "unknown"));
    }

    #[tokio::test]
    async fn consecutive_blocks_and_a_clean_end() {
        let mut input: &[u8] = b"0003abc0000";
        assert_eq!(read_block(&mut input).await.unwrap().as_deref(), Some("abc"));
        assert_eq!(read_block(&mut input).await.unwrap().as_deref(), Some(""));
        assert_eq!(read_block(&mut input).await.unwrap(), None);
    }

    #[tokio::test]
    async fn end_inside_a_prefix_is_an_error() {
        let mut input: &[u8] = b"00";
        assert!(matches!(read_block(&mut input).await, Err(AdbError::Protocol(_))));
    }
}
