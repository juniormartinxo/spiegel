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
    /// Qualquer outro estado do adb (recovery, bootloader, authorizing…),
    /// com o texto do adb como veio.
    #[serde(rename_all = "camelCase")]
    Other { adb_state: String },
}

impl DeviceState {
    fn from_adb(state: &str) -> Self {
        match state {
            "device" => Self::Ready,
            "unauthorized" => Self::Unauthorized,
            "offline" => Self::Offline,
            other => Self::Other { adb_state: other.to_owned() },
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
    let mut tokens = line.split_whitespace().peekable();
    let serial = tokens.next()?.to_owned();
    let mut state = tokens.next()?.to_owned();
    // O único estado com espaço: "no permissions (…); see [url]", que o adb
    // mostra no Linux sem regra do udev. O resto da explicação não atrapalha
    // a busca pelo modelo abaixo.
    if state == "no" && tokens.next_if_eq(&"permissions").is_some() {
        state.push_str(" permissions");
    }
    let state = DeviceState::from_adb(&state);
    let model = tokens
        .find_map(|token| token.strip_prefix("model:"))
        .map(|model| model.replace('_', " "));
    Some(Device { serial, model, state })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_long_adb_list() {
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
    fn unknown_state_becomes_other() {
        let devices = parse_device_list("abc recovery transport_id:1\n");
        assert_eq!(devices[0].state, DeviceState::Other { adb_state: "recovery".into() });
    }

    #[test]
    fn empty_list() {
        assert!(parse_device_list("").is_empty());
    }

    #[test]
    fn no_permissions_state_with_spaces() {
        let line = concat!(
            "0123456789ABCDEF no permissions (user in plugdev group; are your udev rules wrong?); ",
            "see [http://developer.android.com/tools/device.html] usb:1-4 transport_id:2\n",
        );
        let devices = parse_device_list(line);
        assert_eq!(devices[0].serial, "0123456789ABCDEF");
        assert_eq!(devices[0].state, DeviceState::Other { adb_state: "no permissions".into() });
    }
}
