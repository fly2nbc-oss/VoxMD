import { openUrl } from "@tauri-apps/plugin-opener";
import appIcon from "../../src-tauri/icons/128x128.png";
import { useT } from "../i18n/I18nProvider";
import { Modal } from "./Modal";

const GITHUB_URL = "https://github.com/fly2nbc-oss/VoxMD";

interface Props {
  version: string;
  onClose: () => void;
}

export function AboutDialog({ version, onClose }: Props) {
  const { t } = useT();
  return (
    <Modal title={t("about.title")} onClose={onClose} panelClassName="about-dialog">
      <div className="about-brand">
        <img src={appIcon} alt="" width={112} height={112} className="about-app-icon" />
      </div>
      <p className="about-tagline">{t("about.tagline")}</p>
      <p className="about-version">
        <span className="muted-text">{t("about.version")}</span> <span className="mono">{version || "…"}</span>
      </p>
      <div>
        <div className="about-section-label">{t("about.repo")}</div>
        <button
          type="button"
          className="about-repo-link"
          title={GITHUB_URL}
          onClick={() => void openUrl(GITHUB_URL)}
        >
          {GITHUB_URL}
        </button>
      </div>
    </Modal>
  );
}
