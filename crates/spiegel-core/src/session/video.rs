//! O fluxo de vídeo do scrcpy 4.x (`doc/develop.md`, "Video and audio"):
//! o id do codec (u32) e depois cabeçalhos de 12 bytes, que são pacotes de
//! sessão (bit mais alto ligado: largura e altura) ou pacotes de mídia
//! (flags, PTS, tamanho e o conteúdo codificado).

use std::future::Future;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::{EndReason, SessionEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp8,
    Vp9,
}

impl VideoCodec {
    fn from_id(id: u32) -> Option<Self> {
        match &id.to_be_bytes() {
            b"h264" => Some(Self::H264),
            b"h265" => Some(Self::H265),
            b"\0av1" => Some(Self::Av1),
            b"\0vp8" => Some(Self::Vp8),
            b"\0vp9" => Some(Self::Vp9),
            _ => None,
        }
    }
}

/// Um pacote de mídia, com o conteúdo como o codificador do Dispositivo o
/// produziu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPacket {
    /// Dados de configuração do codec (no H.264, o SPS e o PPS), não um quadro.
    pub config: bool,
    pub key_frame: bool,
    /// Em microssegundos. Pacotes de config não têm.
    pub pts: Option<u64>,
    pub data: Vec<u8>,
}

const SESSION_FLAG: u64 = 1 << 63;
const CONFIG_FLAG: u64 = 1 << 62;
const KEY_FRAME_FLAG: u64 = 1 << 61;
const PTS_MASK: u64 = KEY_FRAME_FLAG - 1;

/// Acima disso, o tamanho só pode ser um fluxo corrompido.
const MAX_PACKET_SIZE: usize = 64 * 1024 * 1024;

/// Lê o fluxo e repassa cada pacote como evento, até o fluxo acabar ou
/// `emit` devolver `false` (ninguém mais escuta). Devolve o motivo do fim.
pub(super) async fn forward_stream<R, F, Fut>(mut stream: R, mut emit: F) -> EndReason
where
    R: AsyncRead + Unpin,
    F: FnMut(SessionEvent) -> Fut,
    Fut: Future<Output = bool>,
{
    let Ok(codec_id) = stream.read_u32().await else {
        return EndReason::Disconnected;
    };
    let Some(codec) = VideoCodec::from_id(codec_id) else {
        return protocol_error(format!("unknown video codec id {codec_id:#010x}"));
    };

    let mut header = [0u8; 12];
    loop {
        if stream.read_exact(&mut header).await.is_err() {
            return EndReason::Disconnected;
        }
        let flags_and_pts = u64::from_be_bytes(header[..8].try_into().expect("8 bytes"));
        let tail = u32::from_be_bytes(header[8..].try_into().expect("4 bytes"));

        let event = if flags_and_pts & SESSION_FLAG != 0 {
            // O bit "client resized" (o último do primeiro u32) só importa
            // para telas virtuais redimensionáveis, que o Spiegel ainda não usa.
            let width = (flags_and_pts & 0xffff_ffff) as u32;
            SessionEvent::VideoConfigured { codec, width, height: tail }
        } else {
            let size = tail as usize;
            if size > MAX_PACKET_SIZE {
                return protocol_error(format!("video packet of {size} bytes"));
            }
            let mut data = vec![0u8; size];
            if stream.read_exact(&mut data).await.is_err() {
                return EndReason::Disconnected;
            }
            let config = flags_and_pts & CONFIG_FLAG != 0;
            SessionEvent::VideoPacket(VideoPacket {
                config,
                key_frame: flags_and_pts & KEY_FRAME_FLAG != 0,
                pts: (!config).then_some(flags_and_pts & PTS_MASK),
                data,
            })
        };
        if !emit(event).await {
            return EndReason::Stopped;
        }
    }
}

fn protocol_error(detail: String) -> EndReason {
    EndReason::ProtocolError { detail }
}
