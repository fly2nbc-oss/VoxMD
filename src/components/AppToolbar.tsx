import {
  Captions,
  CircleStop,
  FileAudio2,
  FileText,
  Info,
  ListX,
  Play,
  Rss,
  Settings,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { AppConfig } from "../types";

type MdToggle = "includeMeta" | "includeSummary" | "includeTranscript";

interface Props {
  config: AppConfig;
  storeReady: boolean;
  processing: boolean;
  cancelling: boolean;
  itemCount: number;
  selectedCount: number;
  outputInvalid: boolean;
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
  processing,
  cancelling,
  itemCount,
  selectedCount,
  outputInvalid,
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

  const mdToggles: Array<{
    key: MdToggle;
    icon: typeof FileText;
    label: string;
    title: string;
  }> = [
    {
      key: "includeMeta",
      icon: FileText,
      label: "Markdown: metadata block",
      title: config.includeMeta
        ? "Metadata block on — click to omit from Markdown"
        : "Metadata block off — click to include file / episode info",
    },
    {
      key: "includeSummary",
      icon: Sparkles,
      label: "Markdown: summary",
      title: config.includeSummary
        ? hasApiKey
          ? "Summary (LLM) on — click to disable"
          : "Summary on, but no API key yet (skipped until a key is set in Settings)"
        : "Summary (LLM) off — click to enable",
    },
    {
      key: "includeTranscript",
      icon: Captions,
      label: "Markdown: transcript",
      title: config.includeTranscript
        ? "Transcript on — click to omit from Markdown"
        : "Transcript off — click to include Whisper transcript",
    },
  ];

  return (
    <header className="app-bar">
      <h1 className="app-bar-title">VoxMD</h1>

      <div className="app-bar-actions">
        <button
          type="button"
          className="btn-secondary btn-sm"
          onClick={onPickFiles}
          title="Add audio files"
        >
          <FileAudio2 size={18} aria-hidden />
          <span>Files</span>
        </button>
        <button
          type="button"
          className="btn-secondary btn-sm"
          onClick={onOpenPodcast}
          title="Add podcast episodes from an RSS feed"
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
          disabled={processing || itemCount === 0 || outputInvalid}
          onClick={onStart}
          title={
            outputInvalid
              ? "Enable Transcript or Summary (with API key) in the toolbar"
              : selectedCount > 0
                ? `Start processing ${selectedCount} selected entr${selectedCount === 1 ? "y" : "ies"}`
                : "Start processing all entries in the queue"
          }
        >
          <Play size={18} aria-hidden />
          <span>Start{suffix}</span>
        </button>
        {processing ? (
          <button
            type="button"
            className="icon-btn icon-btn-danger"
            title="Cancel batch (stops the current transcription or download)"
            aria-label="Cancel"
            disabled={cancelling}
            onClick={onCancel}
          >
            <CircleStop size={22} aria-hidden />
          </button>
        ) : null}
      </div>

      <div className="app-bar-end">
        {mdToggles.map(({ key, icon: Icon, label, title }) => (
          <button
            key={key}
            type="button"
            className={`icon-btn${config[key] ? " icon-btn-toggle-on" : ""}`}
            title={title}
            aria-label={label}
            aria-pressed={config[key]}
            disabled={!storeReady || processing}
            onClick={() => onToggleMd(key)}
          >
            <Icon className="icon" size={20} aria-hidden />
          </button>
        ))}
        <button
          type="button"
          className={`icon-btn${config.deleteSourceAfterSuccess ? " icon-btn-toggle-danger" : ""}`}
          title={
            config.deleteSourceAfterSuccess
              ? "Audio deleted after export (Markdown always kept) — click to keep audio"
              : "Audio kept after export — click to delete audio only (Markdown stays)"
          }
          aria-label="Delete audio after success"
          aria-pressed={config.deleteSourceAfterSuccess}
          disabled={!storeReady}
          onClick={onToggleDeleteSource}
        >
          <Trash2 className="icon" size={20} aria-hidden />
        </button>
        <button
          type="button"
          className="icon-btn"
          title="Settings"
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
