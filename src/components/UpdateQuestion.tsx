import { useT } from "../i18n/I18nProvider";
import { Modal } from "./Modal";

interface Props {
  onAnswer: (yes: boolean) => void;
}

/** The one question about updates, asked once the window stands still (R12). Esc counts as "No". */
export function UpdateQuestion({ onAnswer }: Props) {
  const { t } = useT();
  return (
    <Modal title={t("update.askTitle")} onClose={() => onAnswer(false)}>
      <p className="settings-intro">{t("update.askBody")}</p>
      <p className="field-hint">{t("update.askHint")}</p>
      <div className="form-actions">
        <button type="button" className="btn-secondary" onClick={() => onAnswer(false)}>
          {t("update.askNo")}
        </button>
        <button type="button" className="btn-primary" onClick={() => onAnswer(true)}>
          {t("update.askYes")}
        </button>
      </div>
    </Modal>
  );
}
