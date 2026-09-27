import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AdbBanner } from "./components/AdbBanner";
import { DeviceList } from "./components/DeviceList";
import { EmptyState } from "./components/EmptyState";
import { SettingsDialog } from "./components/SettingsDialog";
import { getSnapshot, onSnapshot, type RegistrySnapshot } from "./core";

export default function App() {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<RegistrySnapshot | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    // Assina antes de ler o estado atual, para não perder uma mudança no meio.
    const unlisten = onSnapshot(setSnapshot);
    void getSnapshot().then((current) => current && setSnapshot(current));
    return () => void unlisten.then((stop) => stop());
  }, []);

  const devices = snapshot?.devices ?? [];

  return (
    <div className="app">
      <header className="app-header">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true" />
          <h1>{t("app.title")}</h1>
        </div>
        <button type="button" className="button button-ghost" onClick={() => setSettingsOpen(true)}>
          {t("app.settings")}
        </button>
      </header>

      <main className="app-main">
        {snapshot && <AdbBanner status={snapshot.adb} onOpenSettings={() => setSettingsOpen(true)} />}

        <section className="devices" aria-labelledby="devices-heading">
          <div className="section-heading">
            <h2 id="devices-heading">{t("devices.heading")}</h2>
            {devices.length > 0 && <span className="muted">{t("devices.count", { count: devices.length })}</span>}
          </div>
          {devices.length > 0 ? <DeviceList devices={devices} /> : snapshot?.adb.kind !== "starting" && <EmptyState />}
        </section>
      </main>

      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
