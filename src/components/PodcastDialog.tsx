import { FolderOpen, Loader2, Trash2 } from "lucide-react";
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
  return (
    <Modal title="Add podcast episodes" onClose={onClose}>
      <div className="field">
        <label className="field-label" htmlFor="feedUrl">
          RSS feed URL
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
          Output folder for episode Markdown
        </label>
        <div className="input-with-button">
          <input
            id="podcastDir"
            className="input"
            placeholder="Choose a folder…"
            value={outputDir}
            onChange={(e) => onOutputDirChange(e.target.value)}
            disabled={busy}
          />
          <button type="button" className="btn-secondary btn-sm" onClick={onChooseDir} disabled={busy}>
            <FolderOpen size={16} aria-hidden />
            <span>Choose…</span>
          </button>
        </div>
        <p className="field-hint">
          Audio and Markdown are saved here. The toolbar trash icon can delete the audio after
          export — the Markdown file is always kept.
        </p>
      </div>

      {recents.length > 0 ? (
        <div className="field">
          <span className="field-label">Recent feeds</span>
          <ul className="podcast-recents">
            {recents.map((recent) => (
              <li key={recent.feedUrl} className="podcast-recent-row">
                <button
                  type="button"
                  className="podcast-recent-pick"
                  disabled={busy}
                  title="Use this feed and folder"
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
                  title="Remove from recent list"
                  aria-label={`Remove ${recent.feedTitle || recent.feedUrl}`}
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
          Cancel
        </button>
        <button
          type="button"
          className="btn-primary"
          disabled={busy || !feedUrl.trim() || !outputDir.trim()}
          onClick={onSubmit}
        >
          {busy ? <Loader2 size={14} className="icon spin" aria-hidden /> : null}
          {busy ? " Loading…" : "Add episodes"}
        </button>
      </div>
    </Modal>
  );
}
