//! Dispositivos vistos pelo adb.

use serde::Serialize;

/// Um celular ou tablet Android acessível pelo adb (glossário: Dispositivo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// Nome do modelo (ex.: "Pixel 7"). O adb só informa o modelo de
    /// Dispositivos autorizados.
    pub model: Option<String>,
    pub state: DeviceState,
}

/// Estado do Dispositivo segundo o adb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DeviceState {
    /// Autorizado e pronto para uma Sessão (`device` no adb).
    Ready,
    /// Falta aceitar "Permitir depuração USB" no Dispositivo.
    Unauthorized,
    Offline,
    /// Qualquer outro estado do adb (recovery, bootloader, authorizing…).
    Other { state: String },
}

impl DeviceState {
    fn from_adb(state: &str) -> Self {
        match state {
            "device" => Self::Ready,
            "unauthorized" => Self::Unauthorized,
            "offline" => Self::Offline,
            other => Self::Other { state: other.to_owned() },
        }
    }
}

/// Interpreta a lista no formato longo do adb (`host:track-devices-l`, o
/// mesmo de `adb devices -l`): uma linha por Dispositivo, com o serial, o
/// estado e depois pares `chave:valor` como `model:Pixel_7`.
pub fn parse_device_list(text: &str) -> Vec<Device> {
    text.lines().filter_map(parse_device_line).collect()
}

fn parse_device_line(line: &str) -> Option<Device> {
    let mut tokens = line.split_whitespace();
    let serial = tokens.next()?.to_owned();
    let state = DeviceState::from_adb(tokens.next()?);
    let model = tokens
        .find_map(|token| token.strip_prefix("model:"))
        .map(|model| model.replace('_', " "));
    Some(Device { serial, model, state })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreta_lista_longa_do_adb() {
        let text = "R58M123ABC             device usb:1-4 product:beyond1 model:SM_G973F device:beyond1 transport_id:3\n\
                    emulator-5554          unauthorized transport_id:4\n\
                    0123456789ABCDEF       offline transport_id:5\n";

        assert_eq!(
            parse_device_list(text),
            vec![
                Device {
                    serial: "R58M123ABC".into(),
                    model: Some("SM G973F".into()),
                    state: DeviceState::Ready,
                },
                Device { serial: "emulator-5554".into(), model: None, state: DeviceState::Unauthorized },
                Device { serial: "0123456789ABCDEF".into(), model: None, state: DeviceState::Offline },
            ]
        );
    }

    #[test]
    fn estado_desconhecido_vira_other() {
        let devices = parse_device_list("abc recovery transport_id:1\n");
        assert_eq!(devices[0].state, DeviceState::Other { state: "recovery".into() });
    }

    #[test]
    fn lista_vazia() {
        assert!(parse_device_list("").is_empty());
    }
}
