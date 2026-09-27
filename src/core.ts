// Tipos e chamadas do núcleo (crate spiegel-core), espelhando o que a casca
// Tauri serializa. Mantenha em sincronia com os tipos `Serialize` do Rust.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DeviceState =
  | { kind: "ready" }
  | { kind: "unauthorized" }
  | { kind: "offline" }
  | { kind: "other"; state: string };

export interface Device {
  serial: string;
  model: string | null;
  state: DeviceState;
}

export type AdbProblem = { kind: "notFound"; path: string } | { kind: "failed"; detail: string };

export type AdbStatus =
  | { kind: "starting" }
  | { kind: "ready"; version: number }
  | { kind: "conflict"; serverVersion: number; clientVersion: number }
  | { kind: "reconnecting" }
  | { kind: "unavailable"; problem: AdbProblem };

export interface RegistrySnapshot {
  adb: AdbStatus;
  devices: Device[];
}

export interface AdbSettings {
  customPath: string | null;
  bundledPath: string;
}

export const getSnapshot = () => invoke<RegistrySnapshot | null>("get_snapshot");

export const onSnapshot = (handler: (snapshot: RegistrySnapshot) => void): Promise<UnlistenFn> =>
  listen<RegistrySnapshot>("registry-snapshot", (event) => handler(event.payload));

export const getAdbSettings = () => invoke<AdbSettings>("get_adb_settings");

/** `null` volta ao adb embutido. */
export const setAdbPath = (path: string | null) => invoke<void>("set_adb_path", { path });

/** Só por decisão do usuário: encerra o servidor adb atual e inicia o do Spiegel. */
export const restartAdbServer = () => invoke<void>("restart_adb_server");

/** Nome para exibir: o modelo, ou o serial quando o adb não informa o modelo. */
export const deviceName = (device: Device) => device.model ?? device.serial;
