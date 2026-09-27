import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AdbBanner } from "./components/AdbBanner";
import { DeviceList } from "./components/DeviceList";
import { EmptyState } from "./components/EmptyState";
import { SessionView } from "./components/SessionView";
import { SettingsDialog } from "./components/SettingsDialog";
import { deviceName, getSnapshot, onSnapshot, type RegistrySnapshot } from "./core";

export default function App() {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<RegistrySnapshot | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // Seriais dos Dispositivos com uma visualização de Sessão aberta. A
  // interface oferece uma Sessão de Tela por Dispositivo.
  const [sessions, setSessions] = useState<string[]>([]);

  useEffect(() => {
    // Assina antes de ler o estado atual, para não perder uma mudança no meio.
    const unlisten = onSnapshot(setSnapshot);
    void getSnapshot().then((current) => current && setSnapshot(current));
    return () => void unlisten.then((stop) => stop());
  }, []);

  const devices = snapshot?.devices ?? [];
  const activeSessions: ReadonlySet<string> = new Set(sessions);
  const openSession = (serial: string) =>
    setSessions((current) => (current.includes(serial) ? current : [...current, serial]));
  const closeSession = (serial: string) => setSessions((current) => current.filter((open) => open !== serial));
  const nameOf = (serial: string) => {
    const device = devices.find((candidate) => candidate.serial === serial);
    return device ? deviceName(device) : serial;
  };

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

      <main className="app-main" data-sessions={sessions.length > 0}>
        <div className="app-sidebar">
          {snapshot && <AdbBanner status={snapshot.adb} onOpenSettings={() => setSettingsOpen(true)} />}

          <section className="devices" aria-labelledby="devices-heading">
            <div className="section-heading">
              <h2 id="devices-heading">{t("devices.heading")}</h2>
              {devices.length > 0 && <span className="muted">{t("devices.count", { count: devices.length })}</span>}
            </div>
            {devices.length > 0 ? (
              <DeviceList devices={devices} activeSessions={activeSessions} onStartSession={openSession} />
            ) : (
              // Com o adb com problema, o aviso acima explica; o checklist é sobre
              // cabo e depuração USB, então só faz sentido com o adb pronto.
              snapshot?.adb.kind === "ready" && <EmptyState />
            )}
          </section>
        </div>

        {sessions.map((serial) => (
          <SessionView key={serial} serial={serial} fallbackName={nameOf(serial)} onClose={() => closeSession(serial)} />
        ))}
      </main>

      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
