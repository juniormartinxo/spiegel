import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { startSession, stopSession, type EndReason, type SessionEvent, type StartupPhase } from "../core";
import { ScreenDecoder, type DecoderProblem } from "../video/screen-decoder";

interface Props {
  serial: string;
  /** Nome da lista de Dispositivos, até o Dispositivo informar o dele. */
  fallbackName: string;
  /** Fecha a visualização. Desmontar o componente para a Sessão. */
  onClose(): void;
}

type Status =
  | { kind: "starting"; phase: StartupPhase | "waitingVideo" }
  | { kind: "live" }
  | { kind: "ended"; reason: EndReason }
  | { kind: "decoderProblem"; problem: DecoderProblem }
  /** O pedido para iniciar nem chegou ao núcleo. */
  | { kind: "startFailed"; detail: string };

/** A visualização de uma Sessão de Tela: inicia ao montar, para ao desmontar. */
export function SessionView({ serial, fallbackName, onClose }: Props) {
  const { t } = useTranslation();
  const canvas = useRef<HTMLCanvasElement>(null);
  const [status, setStatus] = useState<Status>({ kind: "starting", phase: "pushingServer" });
  const [deviceName, setDeviceName] = useState<string | null>(null);

  useEffect(() => {
    let id: number | null = null;
    // Parar pode ser pedido antes de o núcleo devolver o id da Sessão (os
    // eventos chegam antes da resposta); nesse caso, para assim que ela vier.
    let stopRequested = false;
    const stop = () => {
      stopRequested = true;
      if (id !== null) void stopSession(id);
    };
    const decoder = new ScreenDecoder(canvas.current!, {
      onFirstFrame: () => setStatus((current) => (current.kind === "starting" ? { kind: "live" } : current)),
      onProblem: (problem) => {
        setStatus({ kind: "decoderProblem", problem });
        // Sem imagem, não há por que manter o servidor rodando no Dispositivo.
        stop();
      },
    });

    const onEvent = (event: SessionEvent) => {
      switch (event.kind) {
        case "phase":
          setStatus({ kind: "starting", phase: event.phase });
          break;
        case "connected":
          setDeviceName(event.deviceName);
          setStatus({ kind: "starting", phase: "waitingVideo" });
          break;
        case "videoConfigured":
          decoder.startCapture(event.codec);
          break;
        case "ended":
          decoder.close();
          // Um problema do decodificador explica melhor o fim do que o
          // "parada" que vem em seguida.
          setStatus((current) => (current.kind === "decoderProblem" ? current : { kind: "ended", reason: event.reason }));
          break;
      }
    };

    startSession(serial, onEvent, (packet) => decoder.push(packet)).then(
      (started) => {
        id = started;
        if (stopRequested) void stopSession(started);
      },
      (error) => setStatus({ kind: "startFailed", detail: String(error) }),
    );
    return () => {
      decoder.close();
      stop();
    };
  }, [serial]);

  const running = status.kind === "starting" || status.kind === "live";

  return (
    <section className="session-view" aria-label={deviceName ?? fallbackName}>
      <header className="session-header">
        <h2>{deviceName ?? fallbackName}</h2>
        <button type="button" className="button" onClick={onClose}>
          {running ? t("session.stop") : t("session.close")}
        </button>
      </header>
      <div className="session-screen" data-live={status.kind === "live"}>
        <canvas ref={canvas} className="session-canvas" />
        {status.kind !== "live" && (
          <div className="session-overlay" role="status">
            <StatusMessage status={status} />
          </div>
        )}
      </div>
    </section>
  );
}

function StatusMessage({ status }: { status: Status }) {
  const { t } = useTranslation();
  switch (status.kind) {
    case "starting":
      return (
        <p className="session-phase">
          <span className="spinner" aria-hidden="true" />
          {t(`session.phase.${status.phase}`)}
        </p>
      );
    case "live":
      return null;
    case "startFailed":
      return <p className="session-problem">{t("session.startFailed", status)}</p>;
    case "decoderProblem":
      return <p className="session-problem">{t(`session.decoder.${status.problem.kind}`, status.problem)}</p>;
    case "ended":
      return <EndMessage reason={status.reason} />;
  }
}

function EndMessage({ reason }: { reason: EndReason }) {
  const { t } = useTranslation();
  return (
    <div className="session-problem">
      <p>
        <strong>{t("session.ended.title")}</strong>
      </p>
      <p>
        {reason.kind === "adbFailed"
          ? t(`adb.problem.${reason.problem.kind}`, reason.problem)
          : t(`session.ended.${reason.kind}`, reason)}
      </p>
      {reason.kind === "serverExited" && reason.output && <pre className="server-output">{reason.output}</pre>}
    </div>
  );
}
