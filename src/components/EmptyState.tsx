import { useTranslation } from "react-i18next";

const CHECKS = ["empty.debugging", "empty.cable", "empty.driver", "empty.unlocked"] as const;

export function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="empty-state">
      <div className="empty-illustration" aria-hidden="true" />
      <h3>{t("empty.title")}</h3>
      <p className="muted">{t("empty.lead")}</p>
      <ul className="checklist">
        {CHECKS.map((key) => (
          <li key={key}>{t(key)}</li>
        ))}
      </ul>
    </div>
  );
}
