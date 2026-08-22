import { FolderOpen, Loader2, Trash2 } from "lucide-react";
import { useT } from "../i18n/I18nProvider";
import type { PodcastRecent } from "../types";
import { Modal } from "./Modal";

interface Props {
  feedUrl: string;
  onFeedUrlChange: (v: string) => void;
  outputDir: string;
  onOutputDirChange: (v: string) => void;
  onChooseDir: () => void;
  recents: PodcastRecent[];
  onApplyRecent: (r: PodcastRecent) => void;
  onRemoveRecent: (feedUrl: string) => void;
  busy: boolean;
  error: string;
  onSubmit: () => void;
  onClose: () => void;
}

export function PodcastDialog({
  feedUrl,
  onFeedUrlChange,
  outputDir,
  onOutputDirChange,
  onChooseDir,
  recents,
  onApplyRecent,
  onRemoveRecent,
  busy,
  error,
  onSubmit,
  onClose,
}: Props) {
  const { t } = useT();
  return (
    <Modal title={t("podcast.title")} onClose={onClose}>
      <div className="field">
        <label className="field-label" htmlFor="feedUrl">
          {t("podcast.feedUrl")}
        </label>
        <input
          id="feedUrl"
          className="input"
          placeholder="https://example.com/feed.xml"
          value={feedUrl}
          onChange={(e) => onFeedUrlChange(e.target.value)}
          disabled={busy}
        />
      </div>

      <div className="field">
        <label className="field-label" htmlFor="podcastDir">
          {t("podcast.outputDir")}
        </label>
        <div className="input-with-button">
          <input
            id="podcastDir"
            className="input"
            placeholder={t("podcast.outputDirPlaceholder")}
            value={outputDir}
            onChange={(e) => onOutputDirChange(e.target.value)}
            disabled={busy}
          />
          <button type="button" className="btn-secondary btn-sm" onClick={onChooseDir} disabled={busy}>
            <FolderOpen size={16} aria-hidden />
            <span>{t("common.choose")}</span>
          </button>
        </div>
        <p className="field-hint">{t("podcast.hint")}</p>
      </div>

      {recents.length > 0 ? (
        <div className="field">
          <span className="field-label">{t("podcast.recents")}</span>
          <ul className="podcast-recents">
            {recents.map((recent) => (
              <li key={recent.feedUrl} className="podcast-recent-row">
                <button
                  type="button"
                  className="podcast-recent-pick"
                  disabled={busy}
                  title={t("podcast.useRecent")}
                  onClick={() => onApplyRecent(recent)}
                >
                  <span className="podcast-recent-title">{recent.feedTitle || recent.feedUrl}</span>
                  <span className="podcast-recent-meta mono">
                    {recent.feedTitle ? `${recent.feedUrl} · ` : null}
                    {recent.outputDir}
                  </span>
                </button>
                <button
                  type="button"
                  className="icon-btn"
                  title={t("podcast.removeRecent")}
                  aria-label={t("podcast.removeRecentAria", {
                    name: recent.feedTitle || recent.feedUrl,
                  })}
                  disabled={busy}
                  onClick={() => onRemoveRecent(recent.feedUrl)}
                >
                  <Trash2 size={15} aria-hidden />
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      {error ? <p className="form-error">{error}</p> : null}

      <div className="form-actions">
        <button type="button" className="btn-secondary" onClick={onClose} disabled={busy}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="btn-primary"
          disabled={busy || !feedUrl.trim() || !outputDir.trim()}
          onClick={onSubmit}
        >
          {busy ? <Loader2 size={14} className="icon spin" aria-hidden /> : null}
          {busy ? t("common.loading") : t("podcast.submit")}
        </button>
      </div>
    </Modal>
  );
}
