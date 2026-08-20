import {
  Captions,
  CircleStop,
  FileAudio2,
  FileText,
  Info,
  List,
  ListX,
  Mic,
  Play,
  Rss,
  Settings,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { AppConfig, AppMode } from "../types";

type MdToggle = "includeMeta" | "includeSummary" | "includeTranscript";

interface Props {
  config: AppConfig;
  storeReady: boolean;
  mode: AppMode;
  processing: boolean;
  dictating: boolean;
  cancelling: boolean;
  itemCount: number;
  selectedCount: number;
  outputInvalid: boolean;
  onModeChange: (mode: AppMode) => void;
  onPickFiles: () => void;
  onOpenPodcast: () => void;
  onRemoveSelected: () => void;
  onStart: () => void;
  onCancel: () => void;
  onToggleMd: (key: MdToggle) => void;
  onToggleDeleteSource: () => void;
  onOpenSettings: () => void;
  onOpenAbout: () => void;
}

export function AppToolbar({
  config,
  storeReady,
  mode,
  processing,
  dictating,
  cancelling,
  itemCount,
  selectedCount,
  outputInvalid,
  onModeChange,
  onPickFiles,
  onOpenPodcast,
  onRemoveSelected,
  onStart,
  onCancel,
  onToggleMd,
  onToggleDeleteSource,
  onOpenSettings,
  onOpenAbout,
}: Props) {
  const hasApiKey = config.apiKey.trim() !== "";
  const suffix = selectedCount > 0 ? ` (${selectedCount})` : "";
  const queueMode = mode === "queue";

  const mdToggles: Array<{
    key: MdToggle;
    icon: typeof FileText;
    name: string;
  }> = [
    { key: "includeMeta", icon: FileText, name: "Metadata" },
    { key: "includeSummary", icon: Sparkles, name: "Summary" },
    { key: "includeTranscript", icon: Captions, name: "Transcript" },
  ];

  return (
    <header className="app-bar">
      <h1 className="app-bar-title">VoxMD</h1>

      <div className="mode-switch" role="tablist" aria-label="App mode">
        <button
          type="button"
          role="tab"
          aria-selected={queueMode}
          className={`mode-switch-btn${queueMode ? " is-active" : ""}`}
          disabled={dictating}
          title={dictating ? "Stop dictation first (Esc)" : "Queue mode (Ctrl+1)"}
          onClick={() => onModeChange("queue")}
        >
          <List size={14} aria-hidden />
          Queue
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={!queueMode}
          className={`mode-switch-btn${queueMode ? "" : " is-active"}`}
          disabled={processing}
          title={processing ? "Stop the batch first (Esc)" : "Dictation mode (Ctrl+2)"}
          onClick={() => onModeChange("dictation")}
        >
          <Mic size={14} aria-hidden />
          Dictation
        </button>
      </div>

      {queueMode ? (
        <div className="app-bar-actions">
          <button
            type="button"
            className="btn-secondary btn-sm"
            onClick={onPickFiles}
            title={
              processing
                ? "Add audio files to the running batch"
                : "Add audio files (Ctrl+O)"
            }
          >
            <FileAudio2 size={18} aria-hidden />
            <span>Files</span>
          </button>
          <button
            type="button"
            className="btn-secondary btn-sm"
            onClick={onOpenPodcast}
            title={
              processing
                ? "Add podcast episodes to the running batch"
                : "Add podcast episodes from an RSS feed"
            }
          >
            <Rss size={18} aria-hidden />
            <span>Podcast</span>
          </button>
          <button
            type="button"
            className="btn-secondary btn-sm"
            disabled={processing || selectedCount === 0}
            onClick={onRemoveSelected}
            title="Remove selected entries from the list"
          >
            <ListX size={18} aria-hidden />
            <span>Remove{suffix}</span>
          </button>
          <button
            type="button"
            className="btn-primary btn-sm"
            disabled={processing || dictating || itemCount === 0 || outputInvalid}
            onClick={onStart}
            title={
              outputInvalid
                ? "Enable Transcript or Summary (with API key) in the toolbar"
                : selectedCount > 0
                  ? `Start processing ${selectedCount} selected entr${selectedCount === 1 ? "y" : "ies"} (F5)`
                  : "Start processing all entries in the queue (F5)"
            }
          >
            <Play size={18} aria-hidden />
            <span>Start{suffix}</span>
          </button>
          {processing ? (
            <button
              type="button"
              className="icon-btn icon-btn-danger"
              title="Cancel batch (Esc)"
              aria-label="Cancel"
              disabled={cancelling}
              onClick={onCancel}
            >
              <CircleStop size={22} aria-hidden />
            </button>
          ) : null}
        </div>
      ) : (
        <div className="app-bar-actions">
          <span className="app-bar-mode-hint">Live microphone transcription</span>
        </div>
      )}

      <div className="app-bar-end">
        {queueMode
          ? mdToggles.map(({ key, icon: Icon, name }) => {
              const on = config[key];
              const title =
                key === "includeSummary" && on && !hasApiKey
                  ? "Summary — on (no API key)"
                  : `${name} — ${on ? "on" : "off"}`;
              return (
                <button
                  key={key}
                  type="button"
                  className={`icon-btn${on ? " icon-btn-toggle-on" : ""}`}
                  title={title}
                  aria-label={name}
                  aria-pressed={on}
                  disabled={!storeReady || processing}
                  onClick={() => onToggleMd(key)}
                >
                  <Icon className="icon" size={20} aria-hidden />
                </button>
              );
            })
          : null}
        {queueMode ? (
          <button
            type="button"
            className={`icon-btn${config.deleteSourceAfterSuccess ? " icon-btn-toggle-danger" : ""}`}
            title={`Delete audio — ${config.deleteSourceAfterSuccess ? "on" : "off"}`}
            aria-label="Delete audio"
            aria-pressed={config.deleteSourceAfterSuccess}
            disabled={!storeReady}
            onClick={onToggleDeleteSource}
          >
            <Trash2 className="icon" size={20} aria-hidden />
          </button>
        ) : null}
        <button
          type="button"
          className="icon-btn"
          title="Settings — Ctrl+,"
          aria-label="Settings"
          onClick={onOpenSettings}
        >
          <Settings className="icon" size={22} aria-hidden />
        </button>
        <button
          type="button"
          className="icon-btn"
          title="About"
          aria-label="About"
          onClick={onOpenAbout}
        >
          <Info className="icon" size={22} aria-hidden />
        </button>
      </div>
    </header>
  );
}
