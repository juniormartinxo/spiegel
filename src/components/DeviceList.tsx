import { useTranslation } from "react-i18next";

import { deviceName, type Device, type DeviceState } from "../core";

export function DeviceList({ devices }: { devices: Device[] }) {
  return (
    <ul className="device-list">
      {devices.map((device) => (
        <DeviceCard key={device.serial} device={device} />
      ))}
    </ul>
  );
}

function DeviceCard({ device }: { device: Device }) {
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
              ? t("devices.state.other", { state: device.state.state })
              : t(`devices.state.${device.state.kind}`)}
          </span>
        </div>
        {device.model && <p className="muted mono">{t("devices.serial", { serial: device.serial })}</p>}
        {hint && <p className="device-hint">{t(hint)}</p>}
      </div>
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
