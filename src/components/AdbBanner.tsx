import type { TFunction } from "i18next";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { restartAdbServer, type AdbProblem, type AdbStatus } from "../core";

interface Props {
  status: AdbStatus;
  onOpenSettings: () => void;
}

/** Aviso sobre o adb. Não aparece quando está tudo certo. */
export function AdbBanner({ status, onOpenSettings }: Props) {
  const { t } = useTranslation();
  const [restarting, setRestarting] = useState(false);

  switch (status.kind) {
    case "ready":
      return null;

    case "starting":
    case "reconnecting":
      return (
        <div className="banner banner-info" role="status">
          <span className="spinner" aria-hidden="true" />
          {t(`adb.${status.kind}`)}
        </div>
      );

    case "conflict":
      return (
        <div className="banner banner-warning" role="alert">
          <div className="banner-text">
            <strong>{t("adb.conflict.title")}</strong>
            <p>{t("adb.conflict.body", { server: status.serverVersion, client: status.clientVersion })}</p>
            <p className="muted">{t("adb.conflict.restartNote")}</p>
          </div>
          <div className="banner-actions">
            <button
              type="button"
              className="button button-primary"
              disabled={restarting}
              onClick={() => {
                setRestarting(true);
                void restartAdbServer().finally(() => setRestarting(false));
              }}
            >
              {t("adb.conflict.restart")}
            </button>
            <button type="button" className="button" onClick={onOpenSettings}>
              {t("adb.conflict.useOther")}
            </button>
          </div>
        </div>
      );

    case "unavailable":
      return (
        <div className="banner banner-error" role="alert">
          <p className="banner-text">{problemText(status.problem, t)}</p>
          <div className="banner-actions">
            <button type="button" className="button" onClick={onOpenSettings}>
              {t("adb.openSettings")}
            </button>
          </div>
        </div>
      );
  }
}

function problemText(problem: AdbProblem, t: TFunction): string {
  return problem.kind === "notFound"
    ? t("adb.problem.notFound", { path: problem.path })
    : t(`adb.problem.${problem.kind}`, { detail: problem.detail });
}
