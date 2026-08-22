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
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n";
import { summaryWouldRun } from "../lib/llmProviders";
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
  const { t, tn } = useT();
  const summaryRuns = summaryWouldRun(config);
  const suffix = selectedCount > 0 ? ` (${selectedCount})` : "";
  const queueMode = mode === "queue";

  const mdToggles: Array<{
    key: MdToggle;
    icon: typeof FileText;
    label: MessageKey;
  }> = [
    { key: "includeMeta", icon: FileText, label: "toolbar.metadata" },
    { key: "includeSummary", icon: Sparkles, label: "toolbar.summary" },
    { key: "includeTranscript", icon: Captions, label: "toolbar.transcript" },
  ];

  return (
    <header className="app-bar">
      <h1 className="app-bar-title">VoxMD</h1>

      <div className="mode-switch" role="tablist" aria-label={t("toolbar.appMode")}>
        <button
          type="button"
          role="tab"
          aria-selected={queueMode}
          className={`mode-switch-btn${queueMode ? " is-active" : ""}`}
          disabled={dictating}
          title={dictating ? t("toolbar.queueBlocked") : t("toolbar.queueTitle")}
          onClick={() => onModeChange("queue")}
        >
          <List size={14} aria-hidden />
          {t("toolbar.queue")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={!queueMode}
          className={`mode-switch-btn${queueMode ? "" : " is-active"}`}
          disabled={processing}
          title={processing ? t("toolbar.dictationBlocked") : t("toolbar.dictationTitle")}
          onClick={() => onModeChange("dictation")}
        >
          <Mic size={14} aria-hidden />
          {t("toolbar.dictation")}
        </button>
      </div>

      {queueMode ? (
        <div className="app-bar-actions">
          <button
            type="button"
            className="btn-secondary btn-sm"
            onClick={onPickFiles}
            title={processing ? t("toolbar.filesTitleRunning") : t("toolbar.filesTitle")}
          >
            <FileAudio2 size={18} aria-hidden />
            <span>{t("toolbar.files")}</span>
          </button>
          <button
            type="button"
            className="btn-secondary btn-sm"
            onClick={onOpenPodcast}
            title={processing ? t("toolbar.podcastTitleRunning") : t("toolbar.podcastTitle")}
          >
            <Rss size={18} aria-hidden />
            <span>{t("toolbar.podcast")}</span>
          </button>
          <button
            type="button"
            className="btn-secondary btn-sm"
            disabled={processing || selectedCount === 0}
            onClick={onRemoveSelected}
            title={t("toolbar.removeTitle")}
          >
            <ListX size={18} aria-hidden />
            <span>
              {t("toolbar.remove")}
              {suffix}
            </span>
          </button>
          <button
            type="button"
            className="btn-primary btn-sm"
            disabled={processing || dictating || itemCount === 0 || outputInvalid}
            onClick={onStart}
            title={
              outputInvalid
                ? t("toolbar.startTitleInvalid")
                : selectedCount > 0
                  ? tn(
                      "toolbar.startTitleSelectedOne",
                      "toolbar.startTitleSelectedMany",
                      selectedCount,
                    )
                  : t("toolbar.startTitleAll")
            }
          >
            <Play size={18} aria-hidden />
            <span>
              {t("toolbar.start")}
              {suffix}
            </span>
          </button>
          {processing ? (
            <button
              type="button"
              className="icon-btn icon-btn-danger"
              title={t("toolbar.cancelTitle")}
              aria-label={t("common.cancel")}
              disabled={cancelling}
              onClick={onCancel}
            >
              <CircleStop size={22} aria-hidden />
            </button>
          ) : null}
        </div>
      ) : (
        <div className="app-bar-actions">
          <span className="app-bar-mode-hint">{t("toolbar.liveHint")}</span>
        </div>
      )}

      <div className="app-bar-end">
        {queueMode
          ? mdToggles.map(({ key, icon: Icon, label }) => {
              const on = config[key];
              const name = t(label);
              const title =
                key === "includeSummary" && on && !summaryRuns
                  ? t("toolbar.summaryNoKey")
                  : t("toolbar.toggleState", {
                      name,
                      state: on ? t("common.on") : t("common.off"),
                    });
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
            title={t("toolbar.deleteAudioState", {
              state: config.deleteSourceAfterSuccess ? t("common.on") : t("common.off"),
            })}
            aria-label={t("toolbar.deleteAudio")}
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
          title={t("toolbar.settingsTitle")}
          aria-label={t("toolbar.settings")}
          onClick={onOpenSettings}
        >
          <Settings className="icon" size={22} aria-hidden />
        </button>
        <button
          type="button"
          className="icon-btn"
          title={t("toolbar.about")}
          aria-label={t("toolbar.about")}
          onClick={onOpenAbout}
        >
          <Info className="icon" size={22} aria-hidden />
        </button>
      </div>
    </header>
  );
}
