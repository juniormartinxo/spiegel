import { useTranslation } from "react-i18next";

import { deviceName, type Device, type DeviceState } from "../core";

interface Props {
  devices: Device[];
  /** Seriais dos Dispositivos com uma Sessão rodando. */
  activeSessions: ReadonlySet<string>;
  onStartSession(serial: string): void;
}

export function DeviceList({ devices, activeSessions, onStartSession }: Props) {
  return (
    <ul className="device-list">
      {devices.map((device) => (
        <DeviceCard
          key={device.serial}
          device={device}
          sessionActive={activeSessions.has(device.serial)}
          onStartSession={() => onStartSession(device.serial)}
        />
      ))}
    </ul>
  );
}

interface CardProps {
  device: Device;
  sessionActive: boolean;
  onStartSession(): void;
}

function DeviceCard({ device, sessionActive, onStartSession }: CardProps) {
  const { t } = useTranslation();
  const hint = stateHint(device.state);

  return (
    <li className="device-card" data-state={device.state.kind}>
      <div className="device-icon" aria-hidden="true" />
      <div className="device-body">
        <div className="device-title">
          <h3>{deviceName(device)}</h3>
          <span className="state-badge">
            {device.state.kind === "other"
              ? t("devices.state.other", { state: device.state.adbState })
              : t(`devices.state.${device.state.kind}`)}
          </span>
          {sessionActive && <span className="session-badge">{t("devices.sessionActive")}</span>}
        </div>
        {device.model && <p className="muted mono">{t("devices.serial", { serial: device.serial })}</p>}
        {hint && <p className="device-hint">{t(hint)}</p>}
      </div>
      {device.state.kind === "ready" && !sessionActive && (
        <button type="button" className="button button-primary device-action" onClick={onStartSession}>
          {t("session.start")}
        </button>
      )}
    </li>
  );
}

function stateHint(state: DeviceState): string | null {
  switch (state.kind) {
    case "unauthorized":
      return "devices.hint.unauthorized";
    case "offline":
      return "devices.hint.offline";
    default:
      return null;
  }
}
