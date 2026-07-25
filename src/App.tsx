import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Store } from "@tauri-apps/plugin-store";
import {
  Check,
  CircleStop,
  FileAudio2,
  FolderOpen,
  Info,
  ListX,
  Loader2,
  Moon,
  Play,
  Rss,
  Settings,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { defaultConfig } from "./defaults";
import type { AppConfig, EpisodeInfo, QueueItem } from "./types";
import appIcon from "../src-tauri/icons/128x128.png";

const STORE_FILE = "voxmd-settings.json";
const CONFIG_KEY = "appConfig";

const GITHUB_URL = "https://github.com/fly2nbc-oss/VoxMD";

const AUDIO_EXTENSIONS = ["mp3", "m4a", "mp4", "wav", "ogg", "flac", "webm", "opus"];

function isAudioPath(p: string): boolean {
  const ext = p.split(".").pop()?.toLowerCase() ?? "";
  return AUDIO_EXTENSIONS.includes(ext);
}

function localItem(path: string): QueueItem {
  return {
    id: path,
    kind: "local",
    source: path,
    displayName: path.split(/[/\\]/).pop() ?? path,
  };
}

/** Explicit field picking also drops keys from older versions (temperature, maxTokens, …). */
function mergeConfig(saved: Partial<AppConfig> | null | undefined): AppConfig {
  const base = defaultConfig();
  if (!saved) return base;
  return {
    apiKey: saved.apiKey?.trim() ?? "",
    apiBaseUrl: saved.apiBaseUrl?.trim() ? saved.apiBaseUrl : base.apiBaseUrl,
    apiModel: saved.apiModel?.trim() ? saved.apiModel : base.apiModel,
    whisperModel: saved.whisperModel?.trim() ? saved.whisperModel : base.whisperModel,
    language: saved.language?.trim() ? saved.language : base.language,
    summaryLanguage: saved.summaryLanguage?.trim()
      ? saved.summaryLanguage.trim()
      : base.summaryLanguage,
    useGpu: saved.useGpu ?? base.useGpu,
    deleteSourceAfterSuccess: saved.deleteSourceAfterSuccess ?? base.deleteSourceAfterSuccess,
    includeMeta: saved.includeMeta ?? base.includeMeta,
    includeSummary: saved.includeSummary ?? base.includeSummary,
    includeTranscript: saved.includeTranscript ?? base.includeTranscript,
    podcastOutputDir: saved.podcastOutputDir ?? base.podcastOutputDir,
  };
}

function toMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function isSummarySystemLanguage(lang: string): boolean {
  return lang.trim().toLowerCase() === "system";
}

interface JobProgressPayload {
  path: string;
  displayName: string;
  stage: string;
  whisperPct?: number;
  downloadPct?: number;
  overall?: { completed: number; total: number; pct: number };
  message?: string;
}

type JobRow = JobProgressPayload;

function badgeForStage(stage: string): { className: string; label: string } {
  switch (stage) {
    case "done":     return { className: "badge-ok",      label: "Done"     };
    case "error":    return { className: "badge-error",   label: "Error"    };
    case "skipped":  return { className: "badge-warn",    label: "Skipped"  };
    case "download": return { className: "badge-llm",     label: "Download" };
    case "whisper":  return { className: "badge-warn",    label: "Whisper"  };
    case "llm":      return { className: "badge-llm",     label: "LLM"      };
    case "queued":   return { className: "badge-neutral", label: "Wait"     };
    default:         return { className: "badge-neutral", label: stage      };
  }
}

function detailsForRow(row: JobRow): string {
  switch (row.stage) {
    case "queued":
      return "Waiting in queue…";
    case "download":
      return row.downloadPct != null && row.downloadPct > 0
        ? `Downloading episode… ${row.downloadPct}%`
        : "Downloading episode…";
    case "whisper":
      return "Transcribing…";
    case "llm":
      return row.message ?? "Summary…";
    case "done":
    case "error":
    case "skipped":
      return row.message ?? "";
    default:
      return row.message ?? "";
  }
}

export default function App() {
  const [theme, setTheme] = useState<"light" | "dark">(() =>
    window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [config, setConfig] = useState<AppConfig>(defaultConfig);
  const [storeReady, setStoreReady] = useState(false);
  const [items, setItems] = useState<QueueItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [jobs, setJobs] = useState<Record<string, JobRow>>({});
  const [processing, setProcessing] = useState(false);
  const [overall, setOverall] = useState<{ completed: number; total: number } | null>(null);
  const [statusMsg, setStatusMsg] = useState("");
  const [modelDownload, setModelDownload] = useState<{ pct: number; model: string } | null>(null);
  const [modelInfos, setModelInfos] = useState<Array<{ name: string; sizeHint: string; cached: boolean }>>([]);
  const [clearingCache, setClearingCache] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [aboutVersion, setAboutVersion] = useState<string>("");
  const [vulkanAvailable, setVulkanAvailable] = useState<boolean | null>(null);
  const [detectedSystemSummaryLang, setDetectedSystemSummaryLang] = useState("");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [saveError, setSaveError] = useState("");
  const [podcastOpen, setPodcastOpen] = useState(false);
  const [feedUrl, setFeedUrl] = useState("");
  const [podcastDir, setPodcastDir] = useState("");
  const [feedBusy, setFeedBusy] = useState(false);
  const [podcastError, setPodcastError] = useState("");
  const [dragActive, setDragActive] = useState(false);
  const settingsWasOpen = useRef(false);
  const storeRef = useRef<Awaited<ReturnType<typeof Store.load>> | null>(null);
  const headerCheckboxRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!aboutOpen) return;
    void getVersion().then(setAboutVersion).catch(() => setAboutVersion("—"));
  }, [aboutOpen]);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    void invoke<{ builtWithVulkan: boolean }>("vulkan_status")
      .then((status) => setVulkanAvailable(status.builtWithVulkan))
      .catch(() => setVulkanAvailable(null));
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const store = await Store.load(STORE_FILE, { autoSave: true, defaults: {} });
        storeRef.current = store;
        const saved = await store.get<AppConfig>(CONFIG_KEY);
        setConfig(mergeConfig(saved ?? undefined));
        setStoreReady(true);
      } catch {
        setStoreReady(true);
      }
    })();
  }, []);

  const refreshModelInfos = useCallback(async () => {
    try {
      const infos = await invoke<Array<{ name: string; sizeHint: string; cached: boolean }>>("list_whisper_models");
      setModelInfos(infos);
    } catch (e) {
      console.warn("list_whisper_models unavailable:", e);
    }
  }, []);

  useEffect(() => {
    if (settingsOpen && !settingsWasOpen.current) {
      refreshModelInfos();
    }
    settingsWasOpen.current = settingsOpen;
  }, [settingsOpen, refreshModelInfos]);

  useEffect(() => {
    if (!settingsOpen) return;
    void invoke<string>("system_summary_language")
      .then(setDetectedSystemSummaryLang)
      .catch(() => setDetectedSystemSummaryLang(""));
  }, [settingsOpen]);

  useEffect(() => {
    if (headerCheckboxRef.current) {
      headerCheckboxRef.current.indeterminate = selected.size > 0 && selected.size < items.length;
    }
  }, [selected, items]);

  const clearCache = useCallback(async () => {
    setClearingCache(true);
    try {
      await invoke("clear_whisper_cache");
      await refreshModelInfos();
    } catch (e) {
      setStatusMsg(toMsg(e));
    } finally {
      setClearingCache(false);
    }
  }, [refreshModelInfos]);

  const saveConfig = useCallback(async (c: AppConfig) => {
    const store = storeRef.current ?? (await Store.load(STORE_FILE, { autoSave: true, defaults: {} }));
    await store.set(CONFIG_KEY, c);
    await store.save();
    setConfig(c);
  }, []);

  useEffect(() => {
    let unlistenProg: (() => void) | undefined;
    let unlistenDone: (() => void) | undefined;

    (async () => {
      unlistenProg = await listen<JobProgressPayload>("job_progress", (e) => {
        const p = e.payload;
        setJobs((prev) => ({
          ...prev,
          [p.path]: { ...p, downloadPct: p.downloadPct ?? prev[p.path]?.downloadPct },
        }));
        if (p.overall) {
          setOverall({ completed: p.overall.completed, total: p.overall.total });
        }
        if (p.message && (p.stage === "done" || p.stage === "error" || p.stage === "skipped")) {
          setStatusMsg(p.message);
        }
      });
      unlistenDone = await listen<{ total: number; cancelled?: boolean; error?: string }>("batch_complete", (e) => {
        setProcessing(false);
        setModelDownload(null);
        if (e.payload.error) setStatusMsg(e.payload.error);
        else if (e.payload.cancelled) setStatusMsg("Batch cancelled.");
        else setStatusMsg("Batch complete.");
      });

      await listen<{ stage: string; model?: string; downloaded?: number; total?: number; pct?: number }>(
        "model_download_progress",
        (e) => {
          const { stage, model, pct } = e.payload;
          if (stage === "ready" || stage === "resolving") {
            setModelDownload(null);
          } else if (stage === "downloading" && model != null && pct != null) {
            setModelDownload({ pct, model });
          }
        },
      );
    })();

    return () => {
      unlistenProg?.();
      unlistenDone?.();
    };
  }, []);

  /** Append new items (deduplicated by id) and queue job rows for them. */
  const addItems = useCallback((added: QueueItem[]) => {
    setItems((prev) => {
      const seen = new Set(prev.map((i) => i.id));
      const fresh = added.filter((i) => !seen.has(i.id));
      if (fresh.length === 0) return prev;
      setJobs((prevJobs) => {
        const next = { ...prevJobs };
        for (const item of fresh) {
          next[item.id] = { path: item.id, displayName: item.displayName, stage: "queued" };
        }
        return next;
      });
      return [...prev, ...fresh];
    });
  }, []);

  // Native drag & drop: the Tauri webview suppresses HTML5 drops and emits
  // its own event carrying real filesystem paths.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    (async () => {
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        const t = event.payload.type;
        if (t === "enter" || t === "over") {
          setDragActive(true);
        } else if (t === "leave") {
          setDragActive(false);
        } else if (t === "drop") {
          setDragActive(false);
          const paths = event.payload.paths;
          const audio = paths.filter(isAudioPath);
          if (audio.length > 0) {
            addItems(audio.map(localItem));
          }
          const ignored = paths.length - audio.length;
          if (ignored > 0) {
            setStatusMsg(
              audio.length > 0
                ? `${audio.length} file(s) added, ${ignored} unsupported item(s) ignored.`
                : "No supported audio files in the drop.",
            );
          } else if (audio.length > 0) {
            setStatusMsg(`${audio.length} file(s) added.`);
          }
        }
      });
      if (cancelled) stop();
      else unlisten = stop;
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addItems]);

  const pickFiles = async () => {
    const sel = await open({
      title: "Audio files",
      multiple: true,
      filters: [
        {
          name: "Audio",
          extensions: AUDIO_EXTENSIONS,
        },
      ],
    });
    if (!sel) return;
    const list = Array.isArray(sel) ? sel : [sel];
    addItems(list.map(localItem));
    setStatusMsg(`${list.length} file(s) added.`);
  };

  const openPodcast = () => {
    setFeedUrl("");
    setPodcastDir(config.podcastOutputDir || "");
    setPodcastError("");
    setPodcastOpen(true);
  };

  const choosePodcastDir = async () => {
    const dir = await open({
      title: "Output folder for episode Markdown files",
      directory: true,
    });
    if (typeof dir === "string" && dir) setPodcastDir(dir);
  };

  const addPodcast = async () => {
    setFeedBusy(true);
    setPodcastError("");
    try {
      const episodes = await invoke<EpisodeInfo[]>("fetch_podcast_feed", { url: feedUrl.trim() });
      const dir = podcastDir;
      addItems(
        episodes.map((ep) => ({
          id: ep.audioUrl,
          kind: "podcast" as const,
          source: ep.audioUrl,
          displayName: ep.date ? `${ep.date} · ${ep.title}` : ep.title,
          episode: {
            feedTitle: ep.feedTitle,
            title: ep.title,
            date: ep.date,
            link: ep.link,
            outputDir: dir,
          },
        })),
      );
      setPodcastOpen(false);
      setStatusMsg(`${episodes.length} episode(s) added.`);
      if (dir !== config.podcastOutputDir) {
        await saveConfig({ ...config, podcastOutputDir: dir });
      }
    } catch (e) {
      setPodcastError(toMsg(e));
    } finally {
      setFeedBusy(false);
    }
  };

  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleSelectAll = (checked: boolean) => {
    setSelected(checked ? new Set(items.map((i) => i.id)) : new Set());
  };

  const removeSelected = () => {
    const count = selected.size;
    if (count === 0) return;
    setItems((prev) => prev.filter((i) => !selected.has(i.id)));
    setJobs((prev) => {
      const next: Record<string, JobRow> = {};
      for (const [k, v] of Object.entries(prev)) {
        if (!selected.has(k)) next[k] = v;
      }
      return next;
    });
    setSelected(new Set());
    setStatusMsg(`${count} entry(ies) removed from the list.`);
  };

  const start = async () => {
    if (!items.length) {
      setStatusMsg("No entries in queue.");
      return;
    }
    try {
      await invoke("start_transcription", { items, config });
      setProcessing(true);
      setSelected(new Set());
      setJobs(() => {
        const next: Record<string, JobRow> = {};
        for (const item of items) {
          next[item.id] = { path: item.id, displayName: item.displayName, stage: "queued" };
        }
        return next;
      });
      setOverall({ completed: 0, total: items.length });
    } catch (e) {
      setStatusMsg(toMsg(e));
    }
  };

  const cancelProcessing = async () => {
    try {
      await invoke("cancel_transcription");
      setStatusMsg("Cancelling…");
    } catch (e) {
      setStatusMsg(toMsg(e));
    }
  };

  const closeSettings = () => {
    setSettingsOpen(false);
    setSaveState("idle");
    setSaveError("");
  };

  const hasApiKey = config.apiKey.trim() !== "";
  // Summary only runs with a key; without one, the transcript must carry the output.
  const outputInvalid = !(config.includeSummary && hasApiKey) && !config.includeTranscript;

  const handleSaveSettings = async () => {
    setSaveState("saving");
    setSaveError("");
    try {
      await saveConfig(config);
      setSaveState("saved");
      window.setTimeout(() => {
        setSettingsOpen(false);
        setSaveState("idle");
      }, 700);
    } catch (e) {
      setSaveState("idle");
      setSaveError(toMsg(e));
    }
  };

  const rows = useMemo(
    () =>
      items.map((item) => ({
        item,
        job: jobs[item.id] ?? { path: item.id, displayName: item.displayName, stage: "queued" },
      })),
    [items, jobs],
  );

  const allSelected = items.length > 0 && selected.size === items.length;
  const overallPct = overall && overall.total > 0 ? (overall.completed / overall.total) * 100 : 0;

  return (
    <div className="app-shell">
      <header className="app-bar">
        <h1 className="app-bar-title">VoxMD</h1>
        <div className="app-bar-actions">
          <button type="button" className="btn-secondary btn-sm" onClick={pickFiles} title="Add audio files">
            <FileAudio2 size={18} aria-hidden />
            <span>Files</span>
          </button>
          <button type="button" className="btn-secondary btn-sm" onClick={openPodcast} title="Add podcast episodes from an RSS feed">
            <Rss size={18} aria-hidden />
            <span>Podcast</span>
          </button>
          <button
            type="button"
            className="btn-secondary btn-sm"
            disabled={processing || selected.size === 0}
            onClick={removeSelected}
            title="Remove selected entries from the list"
          >
            <ListX size={18} aria-hidden />
            <span>Remove{selected.size > 0 ? ` (${selected.size})` : ""}</span>
          </button>
          <button
            type="button"
            className="btn-primary btn-sm"
            disabled={processing || !items.length}
            onClick={start}
            title="Start processing"
          >
            <Play size={18} aria-hidden />
            <span>Start</span>
          </button>
          {processing ? (
            <button
              type="button"
              className="icon-btn icon-btn-danger"
              title="Cancel batch (stops before the next Whisper file after the current work)"
              aria-label="Cancel"
              onClick={cancelProcessing}
            >
              <CircleStop size={22} aria-hidden />
            </button>
          ) : null}
        </div>
        <div className="app-bar-end">
          <button
            type="button"
            className={`icon-btn${config.deleteSourceAfterSuccess ? " icon-btn-toggle-danger" : ""}`}
            title={
              config.deleteSourceAfterSuccess
                ? "Source audio is deleted after successful export — click to keep"
                : "Source audio is kept after export — click to delete after success"
            }
            aria-label="Delete source audio after success"
            aria-pressed={config.deleteSourceAfterSuccess}
            disabled={!storeReady}
            onClick={() =>
              void saveConfig({ ...config, deleteSourceAfterSuccess: !config.deleteSourceAfterSuccess })
            }
          >
            <Trash2 className="icon" size={20} />
          </button>
          <button
            type="button"
            className="icon-btn"
            title={theme === "dark" ? "Light mode" : "Dark mode"}
            aria-label="Theme"
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
          >
            {theme === "dark" ? <Sun className="icon" size={22} /> : <Moon className="icon" size={22} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Settings"
            aria-label="Settings"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className="icon" size={22} />
          </button>
          <button
            type="button"
            className="icon-btn"
            title="About"
            aria-label="About"
            onClick={() => setAboutOpen(true)}
          >
            <Info className="icon" size={22} aria-hidden />
          </button>
        </div>
      </header>

      {dragActive ? (
        <div className="drop-overlay" aria-hidden>
          <div className="drop-overlay-inner">
            <FileAudio2 size={40} aria-hidden />
            <p>Drop audio files to add them to the queue</p>
          </div>
        </div>
      ) : null}

      <main className="content">
        {items.length === 0 ? (
          <p className="empty-title">
            Add audio files (or drop them anywhere in this window) or podcast episodes, then press Start.
          </p>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th style={{ width: 34 }}>
                    <input
                      ref={headerCheckboxRef}
                      type="checkbox"
                      checked={allSelected}
                      disabled={processing || !items.length}
                      onChange={(e) => toggleSelectAll(e.target.checked)}
                      aria-label="Select all entries"
                    />
                  </th>
                  <th style={{ width: "40%" }}>File / Episode</th>
                  <th style={{ width: "90px" }}>Status</th>
                  <th>Details</th>
                </tr>
              </thead>
              <tbody>
                {rows.map(({ item, job }) => {
                  const b = badgeForStage(job.stage);
                  const details = detailsForRow(job);
                  return (
                    <tr key={item.id}>
                      <td>
                        <input
                          type="checkbox"
                          checked={selected.has(item.id)}
                          disabled={processing}
                          onChange={() => toggleSelect(item.id)}
                          aria-label={`Select ${item.displayName}`}
                        />
                      </td>
                      <td className="mono">{item.displayName}</td>
                      <td>
                        <span className={`badge ${b.className}`}>{b.label}</span>
                      </td>
                      <td className="mono" style={{ color: "var(--muted)" }}>
                        {details}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <footer className="meta-bar">
          <span>
            {modelDownload
              ? `Downloading ${modelDownload.model}…`
              : overall
                ? `Overall: ${overall.completed} / ${overall.total} done (MD)`
                : items.length
                  ? `${items.length} queued`
                  : "Empty"}
          </span>
          <div className="progress-track" title={modelDownload ? `Downloading model: ${modelDownload.pct}%` : "Overall progress"}>
            <div
              className="progress-bar"
              style={{ width: `${Math.min(100, modelDownload ? modelDownload.pct : overallPct)}%` }}
            />
          </div>
          <span
            style={{
              maxWidth: "45%",
              display: "flex",
              alignItems: "center",
              justifyContent: "flex-end",
              gap: 6,
              minWidth: 0,
            }}
          >
            {modelDownload ? (
              <span className="mono">{`${modelDownload.pct}%`}</span>
            ) : processing ? (
              <>
                <Loader2
                  size={14}
                  className="icon"
                  aria-hidden
                  style={{ animation: "spin 1s linear infinite", flexShrink: 0 }}
                />
                <span>{statusMsg === "Cancelling…" ? "Cancelling…" : "Running"}</span>
              </>
            ) : (
              <span className="mono" style={{ textAlign: "right" }}>
                {statusMsg}
              </span>
            )}
          </span>
        </footer>
      </main>

      {podcastOpen ? (
        <div className="drawer-overlay about-overlay" role="presentation" onMouseDown={() => setPodcastOpen(false)}>
          <div className="modal-dialog" onMouseDown={(ev) => ev.stopPropagation()}>
            <div className="drawer-header">
              <strong>Add podcast episodes</strong>
              <button type="button" className="icon-btn" title="Close" aria-label="Close" onClick={() => setPodcastOpen(false)}>
                <X size={18} />
              </button>
            </div>
            <div className="drawer-body">
              <div>
                <label className="field-label" htmlFor="feedUrl">
                  RSS feed URL
                </label>
                <input
                  id="feedUrl"
                  className="input"
                  placeholder="https://example.com/feed.xml"
                  value={feedUrl}
                  onChange={(e) => setFeedUrl(e.target.value)}
                  disabled={feedBusy}
                />
              </div>
              <div>
                <label className="field-label">Output folder for episode Markdown</label>
                <div style={{ display: "flex", gap: 6 }}>
                  <input
                    className="input"
                    style={{ flex: 1 }}
                    placeholder="Choose a folder…"
                    value={podcastDir}
                    onChange={(e) => setPodcastDir(e.target.value)}
                    disabled={feedBusy}
                  />
                  <button
                    type="button"
                    className="btn-secondary btn-sm"
                    onClick={choosePodcastDir}
                    disabled={feedBusy}
                    style={{ flexShrink: 0 }}
                  >
                    <FolderOpen size={16} aria-hidden />
                    <span>Choose…</span>
                  </button>
                </div>
                <p style={{ margin: "4px 0 0", fontSize: 11, color: "var(--muted)" }}>
                  Episodes are downloaded temporarily for transcription and deleted afterwards; only the
                  Markdown file is kept here.
                </p>
              </div>
              {podcastError ? (
                <p style={{ margin: 0, fontSize: 12, color: "var(--status-error)" }}>{podcastError}</p>
              ) : null}
              <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
                <button type="button" className="btn-secondary" onClick={() => setPodcastOpen(false)} disabled={feedBusy}>
                  Cancel
                </button>
                <button
                  type="button"
                  className="btn-primary"
                  disabled={feedBusy || !feedUrl.trim() || !podcastDir.trim()}
                  onClick={addPodcast}
                >
                  {feedBusy ? (
                    <Loader2 size={14} className="icon" style={{ animation: "spin 1s linear infinite" }} />
                  ) : null}
                  {feedBusy ? " Loading…" : "Add episodes"}
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      {settingsOpen ? (
        <div className="drawer-overlay" role="presentation" onMouseDown={closeSettings}>
          <aside className="drawer" onMouseDown={(ev) => ev.stopPropagation()}>
            <div className="drawer-header">
              <strong>Settings</strong>
              <button type="button" className="icon-btn" title="Close" aria-label="Close" onClick={closeSettings}>
                <X size={18} />
              </button>
            </div>
            <div className="drawer-body">
              <p style={{ margin: 0, color: "var(--muted)", fontSize: 12 }}>
                Settings are stored locally. API keys never leave this device.
              </p>

              <div>
                <label className="field-label">Markdown output</label>
                <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", marginBottom: 6 }}>
                  <input
                    type="checkbox"
                    checked={config.includeMeta}
                    onChange={(e) => setConfig({ ...config, includeMeta: e.target.checked })}
                  />
                  <span>Metadata block (file / podcast episode info)</span>
                </label>
                <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", marginBottom: 6 }}>
                  <input
                    type="checkbox"
                    checked={config.includeSummary}
                    onChange={(e) => setConfig({ ...config, includeSummary: e.target.checked })}
                  />
                  <span>Summary (LLM)</span>
                </label>
                <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                  <input
                    type="checkbox"
                    checked={config.includeTranscript}
                    onChange={(e) => setConfig({ ...config, includeTranscript: e.target.checked })}
                  />
                  <span>Transcript</span>
                </label>
                {outputInvalid ? (
                  <p style={{ margin: "6px 0 0", fontSize: 11, color: "var(--status-error)" }}>
                    {config.includeSummary
                      ? "Summary requires an API key — enter one below or enable the transcript."
                      : "Enable at least Summary or Transcript — otherwise the output would be empty."}
                  </p>
                ) : !config.includeSummary ? (
                  <p style={{ margin: "6px 0 0", fontSize: 11, color: "var(--muted)" }}>
                    Without the summary, no LLM API access is needed.
                  </p>
                ) : null}
              </div>
              <div>
                <label className="field-label" htmlFor="apiKey">
                  API key
                </label>
                <input
                  id="apiKey"
                  className="input"
                  type="password"
                  autoComplete="off"
                  value={config.apiKey}
                  disabled={!config.includeSummary}
                  onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
                />
                {config.includeSummary && !hasApiKey ? (
                  <p style={{ margin: "4px 0 0", fontSize: 11, color: "var(--status-warn)" }}>
                    No API key — the summary will be skipped.
                  </p>
                ) : null}
              </div>
              <div>
                <label className="field-label" htmlFor="apiBase">
                  API base URL
                </label>
                <input
                  id="apiBase"
                  className="input"
                  value={config.apiBaseUrl}
                  disabled={!config.includeSummary}
                  onChange={(e) => setConfig({ ...config, apiBaseUrl: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label" htmlFor="model">
                  Model
                </label>
                <input
                  id="model"
                  className="input"
                  value={config.apiModel}
                  disabled={!config.includeSummary}
                  onChange={(e) => setConfig({ ...config, apiModel: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label" htmlFor="wmodel">
                  Whisper model
                </label>
                <div style={{ display: "flex", gap: 6 }}>
                  <select
                    id="wmodel"
                    className="input"
                    value={modelInfos.some((m) => m.name === config.whisperModel) ? config.whisperModel : "__custom__"}
                    onChange={(e) => {
                      if (e.target.value !== "__custom__") {
                        setConfig({ ...config, whisperModel: e.target.value });
                      }
                    }}
                    style={{ flex: 1 }}
                  >
                    {modelInfos.map((m) => (
                      <option key={m.name} value={m.name}>
                        {m.name.padEnd(14)} · {m.sizeHint}{m.cached ? " ✓" : ""}
                      </option>
                    ))}
                    {!modelInfos.some((m) => m.name === config.whisperModel) && (
                      <option value="__custom__">{config.whisperModel} (custom path)</option>
                    )}
                  </select>
                  <button
                    type="button"
                    className="btn-secondary btn-sm"
                    title="Delete all cached model files"
                    disabled={clearingCache || modelInfos.every((m) => !m.cached)}
                    onClick={clearCache}
                    style={{ flexShrink: 0, whiteSpace: "nowrap" }}
                  >
                    {clearingCache ? <Loader2 size={13} className="icon" style={{ animation: "spin 1s linear infinite" }} /> : null}
                    {clearingCache ? "Deleting…" : "Clear cache"}
                  </button>
                </div>
                <div style={{ display: "flex", flexWrap: "wrap", gap: "2px 10px", marginTop: 4 }}>
                  {modelInfos.map((m) => (
                    <span key={m.name} style={{ fontSize: 11, color: m.cached ? "var(--status-ok)" : "var(--muted)" }}>
                      {m.cached ? "✓ " : ""}{m.name}
                    </span>
                  ))}
                </div>
                <p style={{ margin: "4px 0 0", fontSize: 11, color: "var(--muted)" }}>
                  Auto-downloaded from HuggingFace (ggerganov/whisper.cpp) on first use. ✓ = cached locally.
                  Or paste an absolute path to a local <code>.bin</code>/<code>.gguf</code> file.
                </p>
                {!modelInfos.some((m) => m.name === config.whisperModel) && (
                  <input
                    className="input"
                    style={{ marginTop: 6 }}
                    placeholder="Absolute path to .bin / .gguf file"
                    value={config.whisperModel}
                    onChange={(e) => setConfig({ ...config, whisperModel: e.target.value })}
                  />
                )}
              </div>
              <div>
                <label className="field-label" htmlFor="lang">
                  Transcription language (ISO, e.g. de)
                </label>
                <input
                  id="lang"
                  className="input"
                  value={config.language}
                  onChange={(e) => setConfig({ ...config, language: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label">Summary language</label>
                <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", marginBottom: 6 }}>
                  <input
                    type="radio"
                    name="summaryLangMode"
                    checked={isSummarySystemLanguage(config.summaryLanguage)}
                    onChange={() => setConfig({ ...config, summaryLanguage: "system" })}
                  />
                  <span>System language</span>
                </label>
                {isSummarySystemLanguage(config.summaryLanguage) && detectedSystemSummaryLang ? (
                  <p style={{ margin: "0 0 8px 22px", fontSize: 11, color: "var(--muted)" }}>
                    Detected: {detectedSystemSummaryLang}
                  </p>
                ) : null}
                <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", marginBottom: 6 }}>
                  <input
                    type="radio"
                    name="summaryLangMode"
                    checked={!isSummarySystemLanguage(config.summaryLanguage)}
                    onChange={() => {
                      const iso = isSummarySystemLanguage(config.summaryLanguage)
                        ? config.language
                        : config.summaryLanguage;
                      setConfig({ ...config, summaryLanguage: iso || "de" });
                    }}
                  />
                  <span>ISO code</span>
                </label>
                {!isSummarySystemLanguage(config.summaryLanguage) ? (
                  <input
                    className="input"
                    aria-label="Summary language ISO code"
                    placeholder="de"
                    value={config.summaryLanguage}
                    onChange={(e) => setConfig({ ...config, summaryLanguage: e.target.value })}
                  />
                ) : null}
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={config.useGpu}
                  disabled={vulkanAvailable === false}
                  onChange={(e) => setConfig({ ...config, useGpu: e.target.checked })}
                />
                <span>Use GPU</span>
                <span
                  className={`badge ${
                    vulkanAvailable === true
                      ? "badge-ok"
                      : vulkanAvailable === false
                        ? "badge-warn"
                        : "badge-neutral"
                  }`}
                  title="Status of this VoxMD binary"
                >
                  {vulkanAvailable === true
                    ? "Vulkan available"
                    : vulkanAvailable === false
                      ? "Vulkan not available"
                      : "Checking Vulkan…"}
                </span>
              </label>
              {saveError ? (
                <p style={{ margin: 0, fontSize: 12, color: "var(--status-error)" }}>{saveError}</p>
              ) : null}
              <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
                <button
                  type="button"
                  className="btn-primary"
                  disabled={!storeReady || outputInvalid || saveState !== "idle"}
                  onClick={handleSaveSettings}
                  style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
                >
                  {saveState === "saved" ? (
                    <>
                      <Check size={15} aria-hidden /> Saved
                    </>
                  ) : saveState === "saving" ? (
                    <>
                      <Loader2 size={14} className="icon" style={{ animation: "spin 1s linear infinite" }} aria-hidden />{" "}
                      Saving…
                    </>
                  ) : (
                    "Save"
                  )}
                </button>
                <button type="button" className="btn-secondary" onClick={() => setConfig(defaultConfig())}>
                  Reset defaults
                </button>
              </div>
            </div>
          </aside>
        </div>
      ) : null}

      {aboutOpen ? (
        <div className="drawer-overlay about-overlay" role="presentation" onMouseDown={() => setAboutOpen(false)}>
          <div className="about-dialog" onMouseDown={(ev) => ev.stopPropagation()}>
            <div className="drawer-header">
              <strong>About VoxMD</strong>
              <button type="button" className="icon-btn" title="Close" aria-label="Close" onClick={() => setAboutOpen(false)}>
                <X size={20} />
              </button>
            </div>
            <div className="drawer-body">
              <div className="about-brand">
                <img src={appIcon} alt="" width={112} height={112} className="about-app-icon" />
              </div>
              <p style={{ margin: 0, color: "var(--muted)", fontSize: 13 }}>
                Transcribe audio to Markdown with local Whisper and your LLM API. Settings and keys stay on this device.
              </p>
              <p style={{ margin: 0, fontSize: 13 }}>
                <span style={{ color: "var(--muted)" }}>Version</span>{" "}
                <span className="mono">{aboutVersion || "…"}</span>
              </p>
              <div style={{ margin: 0 }}>
                <div style={{ color: "var(--muted)", fontSize: 12, marginBottom: 4 }}>GitHub repository</div>
                <button
                  type="button"
                  className="about-repo-link"
                  title={GITHUB_URL}
                  onClick={() => void openUrl(GITHUB_URL)}
                >
                  {GITHUB_URL}
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      <style>{`
        @keyframes spin { to { transform: rotate(360deg); } }
      `}</style>
    </div>
  );
}
