import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { getAdbSettings, setAdbPath, type AdbSettings } from "../core";

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [current, setCurrent] = useState<AdbSettings | null>(null);
  const [useCustom, setUseCustom] = useState(false);
  const [customPath, setCustomPath] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    dialog.current?.showModal();
    void getAdbSettings().then((settings) => {
      setCurrent(settings);
      setUseCustom(settings.customPath !== null);
      setCustomPath(settings.customPath ?? "");
    });
  }, []);

  const save = async () => {
    try {
      await setAdbPath(useCustom ? customPath.trim() || null : null);
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <dialog ref={dialog} className="dialog" onClose={onClose} aria-labelledby="settings-title">
      <form
        method="dialog"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <h2 id="settings-title">{t("settings.title")}</h2>

        <fieldset className="field-group">
          <legend>{t("settings.adbHeading")}</legend>
          <p className="muted">{t("settings.adbLead")}</p>

          <label className="radio">
            <input type="radio" name="adb" checked={!useCustom} onChange={() => setUseCustom(false)} />
            <span>
              {t("settings.bundled")}
              {current && <span className="muted mono path">{current.bundledPath}</span>}
            </span>
          </label>

          <label className="radio">
            <input type="radio" name="adb" checked={useCustom} onChange={() => setUseCustom(true)} />
            <span>{t("settings.custom")}</span>
          </label>
          <input
            type="text"
            className="text-input mono"
            value={customPath}
            placeholder={t("settings.customPlaceholder")}
            disabled={!useCustom}
            spellCheck={false}
            onChange={(event) => setCustomPath(event.target.value)}
          />
        </fieldset>

        {error && <p className="form-error">{t("settings.saveError", { error })}</p>}

        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose}>
            {t("settings.cancel")}
          </button>
          <button type="submit" className="button button-primary">
            {t("settings.save")}
          </button>
        </div>
      </form>
    </dialog>
  );
}
