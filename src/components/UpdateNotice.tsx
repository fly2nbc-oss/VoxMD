import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleArrowUp, X } from "lucide-react";
import { useEffect } from "react";
import { useT } from "../i18n/I18nProvider";
import { releaseUrl } from "../lib/update";

interface Props {
  version: string;
  onClose: () => void;
}

/** §8 Hinweis: top centre over the content, fades after 10 s. Once per newer version. */
export function UpdateNotice({ version, onClose }: Props) {
  const { t } = useT();

  useEffect(() => {
    const timer = window.setTimeout(onClose, 10_000);
    return () => window.clearTimeout(timer);
  }, [onClose]);

  return (
    <div className="notice" role="status">
      <CircleArrowUp size={16} aria-hidden />
      <span>{t("update.notice", { version })}</span>
      <button type="button" className="link-btn" onClick={() => void openUrl(releaseUrl(version))}>
        {t("update.noticeOpen")}
      </button>
      <button
        type="button"
        className="icon-btn"
        title={t("common.close")}
        aria-label={t("common.close")}
        onClick={onClose}
      >
        <X size={16} aria-hidden />
      </button>
    </div>
  );
}
