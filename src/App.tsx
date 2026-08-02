import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { Store } from "@tauri-apps/plugin-store";
import {
  Captions,
  Check,
  CircleStop,
  FileAudio2,
  FileText,
  FolderOpen,
  Info,
  ListX,
  Loader2,
  Play,
  Rss,
  Settings,
  Sparkles,
  Trash2,
  TriangleAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Modal } from "./components/Modal";
import { defaultConfig } from "./defaults";
import type { AppConfig, EpisodeInfo, PodcastRecent, QueueItem } from "./types";
import appIcon from "../src-tauri/icons/128x128.png";

const STORE_FILE = "voxmd-settings.json";
const CONFIG_KEY = "appConfig";
const THEME_KEY = "voxmd-theme";

const GITHUB_URL = "https://github.com/fly2nbc-oss/VoxMD";

type ThemeMode = "light" | "dark" | "system";

function readStoredTheme(): ThemeMode {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === "light" || saved === "dark" || saved === "system") return saved;
  } catch {
    /* ignore */
  }
  return "system";
}

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

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
    podcastRecents: normalizePodcastRecents(saved.podcastRecents),
  };
}

const PODCAST_RECENTS_MAX = 10;

function normalizePodcastRecents(raw: unknown): PodcastRecent[] {
  if (!Array.isArray(raw)) return [];
  const out: PodcastRecent[] = [];
  const seen = new Set<string>();
  for (const entry of raw) {
    if (!entry || typeof entry !== "object") continue;
    const feedUrl = typeof (entry as PodcastRecent).feedUrl === "string" ? (entry as PodcastRecent).feedUrl.trim() : "";
    const outputDir =
      typeof (entry as PodcastRecent).outputDir === "string" ? (entry as PodcastRecent).outputDir.trim() : "";
    if (!feedUrl || !outputDir || seen.has(feedUrl)) continue;
    seen.add(feedUrl);
    const feedTitle =
      typeof (entry as PodcastRecent).feedTitle === "string" && (entry as PodcastRecent).feedTitle!.trim()
        ? (entry as PodcastRecent).feedTitle!.trim()
        : undefined;
    out.push({ feedUrl, outputDir, feedTitle });
    if (out.length >= PODCAST_RECENTS_MAX) break;
  }
  return out;
}

function rememberPodcastRecent(
  config: AppConfig,
  feedUrl: string,
  outputDir: string,
  feedTitle?: string,
): AppConfig {
  const url = feedUrl.trim();
  const dir = outputDir.trim();
  const title = feedTitle?.trim() || undefined;
  const rest = config.podcastRecents.filter((r) => r.feedUrl !== url);
  return {
    ...config,
    podcastOutputDir: dir,
    podcastRecents: [{ feedUrl: url, outputDir: dir, feedTitle: title }, ...rest].slice(0, PODCAST_RECENTS_MAX),
  };
}

/** Path out of the backend's `Saved: <path>` / `Skipped (exists): <path>` message. */
function outputPathOf(row: JobRow): string | null {
  const msg = row.message;
  if (!msg) return null;
  if (row.stage === "done") {
    // Trailing note, e.g. " (audio deletion failed: ...)", is not part of the path.
    return msg.replace(/^Saved:\s*/, "").replace(/\s+\(audio deletion failed:.*$/, "") || null;
  }
  if (row.stage === "skipped" && msg.startsWith("Skipped (exists): ")) {
    return msg.slice("Skipped (exists): ".length) || null;
  }
  return null;
}

function toMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function isSummarySystemLanguage(lang: string): boolean {
  return lang.trim().toLowerCase() === "system";
}

function isTranscriptionAuto(lang: string): boolean {
  const t = lang.trim().toLowerCase();
  return t === "auto" || t === "";
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
    case "download": return { className: "badge-active",  label: "Download" };
    case "whisper":  return { className: "badge-active",  label: "Whisper"  };
    case "llm":      return { className: "badge-active",  label: "LLM"      };
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
      return row.whisperPct != null && row.whisperPct > 0
        ? `Transcribing… ${row.whisperPct}%`
        : "Transcribing…";
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
  const [themeMode, setThemeMode] = useState<ThemeMode>(readStoredTheme);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [config, setConfig] = useState<AppConfig>(defaultConfig);
  const [storeReady, setStoreReady] = useState(false);
  const [items, setItems] = useState<QueueItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [jobs, setJobs] = useState<Record<string, JobRow>>({});
  const [processing, setProcessing] = useState(false);
  /** Real state rather than comparing `statusMsg` to a literal — any later
   *  progress message overwrote that and flipped the UI back to "Running". */
  const [cancelling, setCancelling] = useState(false);
  const [overall, setOverall] = useState<{ completed: number; total: number } | null>(null);
  const [statusMsg, setStatusMsg] = useState("");
  /** Failures of the current batch. The footer only ever shows the newest message,
   *  so without this the earlier ones scroll past unseen. */
  const [errors, setErrors] = useState<Array<{ id: string; displayName: string; message: string }>>(
    [],
  );
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
  /** Mirrors `config` so async callers read the current value, not a stale capture. */
  const configRef = useRef(config);
  /** Last persisted config, used to discard unsaved drawer edits on close. */
  const savedConfigRef = useRef(config);
  const saveTimerRef = useRef<number | undefined>(undefined);
  /** Read from callbacks that must not re-subscribe when `processing` flips. */
  const isProcessingRef = useRef(false);

  useEffect(() => {
    configRef.current = config;
  }, [config]);

  useEffect(() => {
    isProcessingRef.current = processing;
  }, [processing]);

  useEffect(() => {
    try {
      localStorage.setItem(THEME_KEY, themeMode);
    } catch {
      /* ignore */
    }
    const apply = () => {
      const resolved = themeMode === "system" ? (systemPrefersDark() ? "dark" : "light") : themeMode;
      document.documentElement.setAttribute("data-theme", resolved);
    };
    apply();
    if (themeMode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => apply();
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [themeMode]);

  useEffect(() => {
    void invoke<{
      builtWithVulkan: boolean;
      loaderAvailable: boolean;
      available: boolean;
    }>("vulkan_status")
      .then((status) => setVulkanAvailable(status.available))
      .catch(() => setVulkanAvailable(null));
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const store = await Store.load(STORE_FILE, { autoSave: true, defaults: {} });
        storeRef.current = store;
        const saved = await store.get<AppConfig>(CONFIG_KEY);
        const merged = mergeConfig(saved ?? undefined);
        configRef.current = merged;
        savedConfigRef.current = merged;
        setConfig(merged);
      } catch (e) {
        // Say so rather than silently reverting to defaults — the next save would
        // otherwise overwrite the settings file the user still has on disk.
        setStatusMsg(`Settings could not be loaded (${toMsg(e)}). Using defaults.`);
      } finally {
        setStoreReady(true);
      }
    })();
  }, []);

  // Recover the running state after a reload: the backend owns it, and without
  // this the UI could sit disabled behind a batch that had already finished.
  useEffect(() => {
    void invoke<boolean>("processing_state")
      .then(setProcessing)
      .catch(() => {});
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
    if (!aboutOpen) return;
    void getVersion().then(setAboutVersion).catch(() => setAboutVersion("—"));
  }, [aboutOpen]);

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

  /** Persists a config change. Accepts an updater so callers that run after an
   *  `await` cannot write back a `config` captured before it. */
  const saveConfig = useCallback(
    async (update: AppConfig | ((prev: AppConfig) => AppConfig)) => {
      const next = typeof update === "function" ? update(configRef.current) : update;
      configRef.current = next;
      savedConfigRef.current = next;
      setConfig(next);
      const store =
        storeRef.current ?? (await Store.load(STORE_FILE, { autoSave: true, defaults: {} }));
      await store.set(CONFIG_KEY, next);
      await store.save();
    },
    [],
  );

  useEffect(() => {
    // Collected rather than assigned one-by-one: `await listen(...)` can resolve
    // after unmount, and the previous version both dropped the third handle and
    // leaked the first two in that case.
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const track = (stop: () => void) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    };

    (async () => {
      track(
        await listen<JobProgressPayload>("job_progress", (e) => {
          const p = e.payload;
          setJobs((prev) => ({
            ...prev,
            [p.path]: {
              ...p,
              downloadPct: p.downloadPct ?? prev[p.path]?.downloadPct,
              whisperPct: p.whisperPct ?? prev[p.path]?.whisperPct,
            },
          }));
          if (p.overall) {
            setOverall({ completed: p.overall.completed, total: p.overall.total });
          }
          if (p.stage === "error") {
            setErrors((prev) => [
              ...prev.filter((x) => x.id !== p.path),
              { id: p.path, displayName: p.displayName, message: p.message ?? "Failed." },
            ]);
          }
          if (p.message && (p.stage === "done" || p.stage === "error" || p.stage === "skipped")) {
            setStatusMsg(p.message);
          }
        }),
      );

      track(
        await listen<{ total: number; cancelled?: boolean; error?: string }>(
          "batch_complete",
          (e) => {
            setProcessing(false);
            setCancelling(false);
            setModelDownload(null);
            if (e.payload.error) setStatusMsg(e.payload.error);
            else if (e.payload.cancelled) setStatusMsg("Batch cancelled.");
            else setStatusMsg("Batch complete.");
          },
        ),
      );

      track(
        await listen<{
          stage: string;
          model?: string;
          downloaded?: number;
          total?: number;
          pct?: number;
        }>("model_download_progress", (e) => {
          const { stage, model, pct } = e.payload;
          if (stage === "ready" || stage === "resolving") {
            setModelDownload(null);
          } else if (stage === "downloading" && model != null && pct != null) {
            setModelDownload({ pct, model });
          }
        }),
      );
    })();

    return () => {
      cancelled = true;
      for (const stop of unlisteners) stop();
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
    // The finished batch's tally no longer describes the queue; leaving it set
    // kept "Overall: 12 / 12 done" on screen after adding new files.
    setOverall((cur) => (isProcessingRef.current ? cur : null));
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
    try {
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
    } catch (e) {
      // An unhandled rejection here just made the button look dead.
      setStatusMsg(`Could not open the file picker: ${toMsg(e)}`);
    }
  };

  const openPodcast = () => {
    setFeedUrl("");
    setPodcastDir(config.podcastOutputDir || "");
    setPodcastError("");
    setPodcastOpen(true);
  };

  const choosePodcastDir = async () => {
    try {
      const dir = await open({
        title: "Output folder for episode Markdown files",
        directory: true,
      });
      if (typeof dir === "string" && dir) setPodcastDir(dir);
    } catch (e) {
      setPodcastError(`Could not open the folder picker: ${toMsg(e)}`);
    }
  };

  const addPodcast = async () => {
    setFeedBusy(true);
    setPodcastError("");
    try {
      const url = feedUrl.trim();
      const dir = podcastDir.trim();
      const episodes = await invoke<EpisodeInfo[]>("fetch_podcast_feed", { url });
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
      // Updater form: `config` here was captured before the feed request above.
      await saveConfig((prev) => rememberPodcastRecent(prev, url, dir, episodes[0]?.feedTitle));
    } catch (e) {
      setPodcastError(toMsg(e));
    } finally {
      setFeedBusy(false);
    }
  };

  const applyPodcastRecent = (recent: PodcastRecent) => {
    setFeedUrl(recent.feedUrl);
    setPodcastDir(recent.outputDir);
    setPodcastError("");
  };

  const removePodcastRecent = (feedUrl: string) => {
    void saveConfig((prev) => ({
      ...prev,
      podcastRecents: prev.podcastRecents.filter((r) => r.feedUrl !== feedUrl),
    }));
  };

  /** Opens a produced Markdown file, or reveals it if no handler is registered. */
  const openResult = async (path: string) => {
    try {
      await openPath(path);
    } catch {
      try {
        await revealItemInDir(path);
      } catch (e) {
        setStatusMsg(`Could not open ${path}: ${toMsg(e)}`);
      }
    }
  };

  const revealResult = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (e) {
      setStatusMsg(`Could not open the folder: ${toMsg(e)}`);
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
    setOverall(null);
    setStatusMsg(`${count} entr${count === 1 ? "y" : "ies"} removed from the list.`);
  };

  const start = async () => {
    // Starting before the store resolves would send defaultConfig() — no API key,
    // default model — instead of the user's settings.
    if (!storeReady) {
      setStatusMsg("Settings are still loading…");
      return;
    }
    if (!items.length) {
      setStatusMsg("No entries in queue.");
      return;
    }
    // Selection scopes the batch; with nothing checked, process the full queue.
    const toProcess = selected.size > 0 ? items.filter((i) => selected.has(i.id)) : items;
    if (!toProcess.length) {
      setStatusMsg("No entries selected.");
      return;
    }
    // Seed the rows *before* invoking. The backend starts emitting job_progress
    // as soon as the command returns, and writing this afterwards discarded
    // events that had already landed — files skipped as "already exists" stayed
    // on "Wait" for the rest of the session.
    setProcessing(true);
    setCancelling(false);
    setSelected(new Set());
    setErrors([]);
    setStatusMsg("");
    // Merged into the previous map, not replacing it, so rows finished by an
    // earlier run keep their result instead of reverting to "Wait".
    setJobs((prev) => {
      const next = { ...prev };
      for (const item of toProcess) {
        next[item.id] = { path: item.id, displayName: item.displayName, stage: "queued" };
      }
      return next;
    });
    setOverall({ completed: 0, total: toProcess.length });

    try {
      await invoke("start_transcription", { items: toProcess, config });
    } catch (e) {
      setProcessing(false);
      setCancelling(false);
      setOverall(null);
      setStatusMsg(toMsg(e));
    }
  };

  const cancelProcessing = async () => {
    setCancelling(true);
    try {
      await invoke("cancel_transcription");
      setStatusMsg("Cancelling — finishing the current step…");
    } catch (e) {
      setCancelling(false);
      setStatusMsg(toMsg(e));
    }
  };

  const closeSettings = () => {
    // Discard unsaved edits. They previously stayed in the live config: closing the
    // drawer still applied them to the next run, and the next toolbar toggle wrote
    // them to disk. "Reset defaults" followed by any toggle wiped the saved feeds.
    setConfig(savedConfigRef.current);
    configRef.current = savedConfigRef.current;
    setSettingsOpen(false);
    setSaveState("idle");
    setSaveError("");
  };

  const hasApiKey = config.apiKey.trim() !== "";
  // Summary only runs with a key; without one, the transcript must carry the output.
  const outputInvalid = !(config.includeSummary && hasApiKey) && !config.includeTranscript;

  const toggleMdOutput = (key: "includeMeta" | "includeSummary" | "includeTranscript") => {
    const next = { ...config, [key]: !config[key] };
    if (key !== "includeMeta") {
      const summaryWouldRun = next.includeSummary && next.apiKey.trim() !== "";
      if (!summaryWouldRun && !next.includeTranscript) {
        setStatusMsg(
          next.includeSummary
            ? "Need an API key or Transcript — otherwise the Markdown would be empty."
            : "Enable at least Summary or Transcript — otherwise the Markdown would be empty.",
        );
        return;
      }
    }
    void saveConfig(next);
  };

  const handleSaveSettings = async () => {
    setSaveState("saving");
    setSaveError("");
    try {
      await saveConfig(config);
      setSaveState("saved");
      // Tracked so reopening the drawer within the delay does not close it again.
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = window.setTimeout(() => {
        setSettingsOpen(false);
        setSaveState("idle");
      }, 700);
    } catch (e) {
      setSaveState("idle");
      setSaveError(toMsg(e));
    }
  };

  useEffect(() => () => window.clearTimeout(saveTimerRef.current), []);

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
            disabled={processing || !items.length || outputInvalid}
            onClick={start}
            title={
              outputInvalid
                ? "Enable Transcript or Summary (with API key) in the toolbar"
                : selected.size > 0
                  ? `Start processing ${selected.size} selected entr${selected.size === 1 ? "y" : "ies"}`
                  : "Start processing all entries in the queue"
            }
          >
            <Play size={18} aria-hidden />
            <span>Start{selected.size > 0 ? ` (${selected.size})` : ""}</span>
          </button>
          {processing ? (
            <button
              type="button"
              className="icon-btn icon-btn-danger"
              title="Cancel batch (stops the current transcription or download)"
              aria-label="Cancel"
              disabled={cancelling}
              onClick={cancelProcessing}
            >
              <CircleStop size={22} aria-hidden />
            </button>
          ) : null}
        </div>
        <div className="app-bar-end">
          <button
            type="button"
            className={`icon-btn${config.includeMeta ? " icon-btn-toggle-on" : ""}`}
            title={
              config.includeMeta
                ? "Metadata block on — click to omit from Markdown"
                : "Metadata block off — click to include file / episode info"
            }
            aria-label="Markdown: metadata block"
            aria-pressed={config.includeMeta}
            disabled={!storeReady || processing}
            onClick={() => toggleMdOutput("includeMeta")}
          >
            <FileText className="icon" size={20} />
          </button>
          <button
            type="button"
            className={`icon-btn${config.includeSummary ? " icon-btn-toggle-on" : ""}`}
            title={
              config.includeSummary
                ? hasApiKey
                  ? "Summary (LLM) on — click to disable"
                  : "Summary on, but no API key yet (skipped until a key is set in Settings)"
                : "Summary (LLM) off — click to enable"
            }
            aria-label="Markdown: summary"
            aria-pressed={config.includeSummary}
            disabled={!storeReady || processing}
            onClick={() => toggleMdOutput("includeSummary")}
          >
            <Sparkles className="icon" size={20} />
          </button>
          <button
            type="button"
            className={`icon-btn${config.includeTranscript ? " icon-btn-toggle-on" : ""}`}
            title={
              config.includeTranscript
                ? "Transcript on — click to omit from Markdown"
                : "Transcript off — click to include Whisper transcript"
            }
            aria-label="Markdown: transcript"
            aria-pressed={config.includeTranscript}
            disabled={!storeReady || processing}
            onClick={() => toggleMdOutput("includeTranscript")}
          >
            <Captions className="icon" size={20} />
          </button>
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
            onClick={() =>
              void saveConfig((prev) => ({
                ...prev,
                deleteSourceAfterSuccess: !prev.deleteSourceAfterSuccess,
              }))
            }
          >
            <Trash2 className="icon" size={20} />
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

      {errors.length > 0 ? (
        <section className="error-panel" role="alert" aria-label="Failed entries">
          <div className="error-panel-head">
            <TriangleAlert size={16} aria-hidden />
            <strong>
              {errors.length} {errors.length === 1 ? "entry" : "entries"} failed
            </strong>
            <button
              type="button"
              className="icon-btn"
              title="Dismiss"
              aria-label="Dismiss error list"
              onClick={() => setErrors([])}
            >
              <X size={16} aria-hidden />
            </button>
          </div>
          <ul className="error-panel-list">
            {errors.map((err) => (
              <li key={err.id}>
                <span className="error-panel-name">{err.displayName}</span>
                <span className="error-panel-msg mono">{err.message}</span>
              </li>
            ))}
          </ul>
        </section>
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
                  <th scope="col" style={{ width: "40%" }}>
                    File / Episode
                  </th>
                  <th scope="col" style={{ width: "90px" }}>
                    Status
                  </th>
                  <th scope="col">Details</th>
                  <th scope="col" style={{ width: 72 }}>
                    <span className="visually-hidden">Output</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map(({ item, job }) => {
                  const b = badgeForStage(job.stage);
                  const details = detailsForRow(job);
                  const outputPath = outputPathOf(job);
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
                      <td className="mono details-cell">{details}</td>
                      <td className="row-actions">
                        {outputPath ? (
                          <>
                            <button
                              type="button"
                              className="icon-btn"
                              title={`Open ${outputPath}`}
                              aria-label={`Open the Markdown file for ${item.displayName}`}
                              onClick={() => void openResult(outputPath)}
                            >
                              <FileText size={16} aria-hidden />
                            </button>
                            <button
                              type="button"
                              className="icon-btn"
                              title="Show in file manager"
                              aria-label={`Show the output folder for ${item.displayName}`}
                              onClick={() => void revealResult(outputPath)}
                            >
                              <FolderOpen size={16} aria-hidden />
                            </button>
                          </>
                        ) : null}
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
          <div
            className="progress-track"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(modelDownload ? modelDownload.pct : overallPct)}
            aria-label={modelDownload ? "Model download progress" : "Overall batch progress"}
            title={modelDownload ? `Downloading model: ${modelDownload.pct}%` : "Overall progress"}
          >
            <div
              className="progress-bar"
              style={{ width: `${Math.min(100, modelDownload ? modelDownload.pct : overallPct)}%` }}
            />
          </div>
          {/* Announced to screen readers: the status line is the only feedback
              channel for stage changes, results and errors. */}
          <span className="status-slot" aria-live="polite" aria-atomic="true">
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
                <span>{cancelling ? "Cancelling…" : "Running"}</span>
              </>
            ) : (
              <span className="mono status-text">{statusMsg}</span>
            )}
          </span>
        </footer>
      </main>

      {podcastOpen ? (
        <Modal title="Add podcast episodes" onClose={() => setPodcastOpen(false)}>
          <>
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
                  Audio and Markdown are saved here. The toolbar trash icon can delete the audio after
                  export — the Markdown file is always kept.
                </p>
              </div>
              {config.podcastRecents.length > 0 ? (
                <div>
                  <label className="field-label">Recent feeds</label>
                  <ul className="podcast-recents">
                    {config.podcastRecents.map((recent) => (
                      <li key={recent.feedUrl} className="podcast-recent-row">
                        <button
                          type="button"
                          className="podcast-recent-pick"
                          disabled={feedBusy}
                          title="Use this feed and folder"
                          onClick={() => applyPodcastRecent(recent)}
                        >
                          <span className="podcast-recent-title">
                            {recent.feedTitle || recent.feedUrl}
                          </span>
                          <span className="podcast-recent-meta mono">
                            {recent.feedTitle ? recent.feedUrl : null}
                            {recent.feedTitle ? " · " : null}
                            {recent.outputDir}
                          </span>
                        </button>
                        <button
                          type="button"
                          className="icon-btn"
                          title="Remove from recent list"
                          aria-label={`Remove ${recent.feedTitle || recent.feedUrl}`}
                          disabled={feedBusy}
                          onClick={() => removePodcastRecent(recent.feedUrl)}
                        >
                          <Trash2 size={15} aria-hidden />
                        </button>
                      </li>
                    ))}
                  </ul>
                </div>
              ) : null}
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
          </>
        </Modal>
      ) : null}

      {settingsOpen ? (
        <Modal title="Settings" onClose={closeSettings} variant="drawer">
          <>
              <section className="settings-section">
                <h2 className="settings-section-title">Summary (LLM)</h2>
                <p className="field-hint">
                  OpenAI-compatible chat API for the Markdown summary. Only used when Summary is enabled in the
                  toolbar. Without a key, transcription still works; the summary section is skipped.
                </p>
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
                    <p className="field-hint field-hint-warn">Enter a key to generate summaries.</p>
                  ) : !config.includeSummary ? (
                    <p className="field-hint">Summary is off in the toolbar — these fields are unused.</p>
                  ) : null}
                </div>
                <div>
                  <label className="field-label" htmlFor="apiBase">
                    API base URL
                  </label>
                  <input
                    id="apiBase"
                    className="input"
                    placeholder="https://api.deepseek.com"
                    value={config.apiBaseUrl}
                    disabled={!config.includeSummary}
                    onChange={(e) => setConfig({ ...config, apiBaseUrl: e.target.value })}
                  />
                  <p className="field-hint">Endpoint root, without <code>/v1/chat/completions</code>.</p>
                </div>
                <div>
                  <label className="field-label" htmlFor="model">
                    Model
                  </label>
                  <input
                    id="model"
                    className="input"
                    placeholder="deepseek-v4-pro"
                    value={config.apiModel}
                    disabled={!config.includeSummary}
                    onChange={(e) => setConfig({ ...config, apiModel: e.target.value })}
                  />
                  <p className="field-hint">Model id as expected by that provider.</p>
                </div>
                <div>
                  <label className="field-label">Summary language</label>
                  <p className="field-hint">Language of the written summary (not the spoken audio).</p>
                  <div className="lang-option-row">
                    <label className="lang-radio">
                      <input
                        type="radio"
                        name="summaryLangMode"
                        checked={isSummarySystemLanguage(config.summaryLanguage)}
                        onChange={() => setConfig({ ...config, summaryLanguage: "system" })}
                        disabled={!config.includeSummary}
                      />
                      <span>System language</span>
                      {isSummarySystemLanguage(config.summaryLanguage) && detectedSystemSummaryLang ? (
                        <span className="lang-detected">({detectedSystemSummaryLang})</span>
                      ) : null}
                    </label>
                    <label className="lang-radio">
                      <input
                        type="radio"
                        name="summaryLangMode"
                        checked={!isSummarySystemLanguage(config.summaryLanguage)}
                        disabled={!config.includeSummary}
                        onChange={() => {
                          const fromWhisper = isTranscriptionAuto(config.language)
                            ? ""
                            : config.language;
                          const iso = isSummarySystemLanguage(config.summaryLanguage)
                            ? fromWhisper || detectedSystemSummaryLang || "de"
                            : config.summaryLanguage;
                          setConfig({ ...config, summaryLanguage: iso || "de" });
                        }}
                      />
                      <span>ISO code</span>
                    </label>
                    <input
                      className="input lang-iso-input"
                      aria-label="Summary language ISO code"
                      placeholder="de"
                      disabled={!config.includeSummary}
                      value={isSummarySystemLanguage(config.summaryLanguage) ? "" : config.summaryLanguage}
                      onChange={(e) => setConfig({ ...config, summaryLanguage: e.target.value || "de" })}
                      onFocus={() => {
                        if (isSummarySystemLanguage(config.summaryLanguage)) {
                          const fromWhisper = isTranscriptionAuto(config.language) ? "" : config.language;
                          setConfig({
                            ...config,
                            summaryLanguage: fromWhisper || detectedSystemSummaryLang || "de",
                          });
                        }
                      }}
                    />
                  </div>
                </div>
              </section>

              <section className="settings-section">
                <h2 className="settings-section-title">Transcription (Whisper)</h2>
                <p className="field-hint">
                  Local speech-to-text on this device. Produces the transcript section of the Markdown output.
                </p>
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
                        if (e.target.value === "__custom__") {
                          if (modelInfos.some((m) => m.name === config.whisperModel)) {
                            setConfig({ ...config, whisperModel: "" });
                          }
                          return;
                        }
                        setConfig({ ...config, whisperModel: e.target.value });
                      }}
                      style={{ flex: 1 }}
                    >
                      {modelInfos.map((m) => (
                        <option key={m.name} value={m.name}>
                          {m.name.padEnd(14)} · {m.sizeHint}{m.cached ? " ✓" : ""}
                        </option>
                      ))}
                      <option value="__custom__">Custom path…</option>
                    </select>
                    <button
                      type="button"
                      className="btn-secondary btn-sm"
                      title="Delete all downloaded Whisper models from the local cache"
                      disabled={clearingCache || modelInfos.every((m) => !m.cached)}
                      onClick={clearCache}
                      style={{ flexShrink: 0, whiteSpace: "nowrap" }}
                    >
                      {clearingCache ? <Loader2 size={13} className="icon" style={{ animation: "spin 1s linear infinite" }} /> : null}
                      {clearingCache ? "Deleting…" : "Clear cache"}
                    </button>
                  </div>
                  {!modelInfos.some((m) => m.name === config.whisperModel) ? (
                    <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                      <input
                        className="input"
                        style={{ flex: 1 }}
                        placeholder="/absolute/path/to/model.bin"
                        value={config.whisperModel}
                        onChange={(e) => setConfig({ ...config, whisperModel: e.target.value })}
                      />
                      <button
                        type="button"
                        className="btn-secondary btn-sm"
                        title="Choose a local .bin or .gguf model file"
                        onClick={async () => {
                          const file = await open({
                            title: "Whisper model file",
                            multiple: false,
                            filters: [{ name: "Whisper model", extensions: ["bin", "gguf"] }],
                          });
                          if (typeof file === "string" && file) {
                            setConfig({ ...config, whisperModel: file });
                          }
                        }}
                        style={{ flexShrink: 0 }}
                      >
                        <FolderOpen size={16} aria-hidden />
                        <span>Choose…</span>
                      </button>
                    </div>
                  ) : null}
                  <p className="field-hint">
                    {modelInfos.some((m) => m.name === config.whisperModel)
                      ? "Larger models are slower but usually more accurate. Named models download on first use (✓ = already cached)."
                      : "Point to your own Whisper weights (.bin / .gguf)."}
                  </p>
                </div>
                <div>
                  <label className="field-label">Transcription language</label>
                  <p className="field-hint">Spoken language in the audio. Auto-detect works well; ISO is faster when you know it.</p>
                  <div className="lang-option-row">
                    <label className="lang-radio">
                      <input
                        type="radio"
                        name="transcriptionLangMode"
                        checked={isTranscriptionAuto(config.language)}
                        onChange={() => setConfig({ ...config, language: "auto" })}
                      />
                      <span>Auto-detect</span>
                    </label>
                    <label className="lang-radio">
                      <input
                        type="radio"
                        name="transcriptionLangMode"
                        checked={!isTranscriptionAuto(config.language)}
                        onChange={() => {
                          const iso = isTranscriptionAuto(config.language) ? "de" : config.language;
                          setConfig({ ...config, language: iso || "de" });
                        }}
                      />
                      <span>ISO code</span>
                    </label>
                    <input
                      className="input lang-iso-input"
                      aria-label="Transcription language ISO code"
                      placeholder="de"
                      value={isTranscriptionAuto(config.language) ? "" : config.language}
                      onChange={(e) => setConfig({ ...config, language: e.target.value || "de" })}
                      onFocus={() => {
                        if (isTranscriptionAuto(config.language)) {
                          setConfig({ ...config, language: "de" });
                        }
                      }}
                    />
                  </div>
                </div>
                <div>
                  <label className="field-label">GPU</label>
                  <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: vulkanAvailable === false ? "default" : "pointer" }}>
                    <input
                      type="checkbox"
                      checked={config.useGpu}
                      disabled={vulkanAvailable === false}
                      onChange={(e) => setConfig({ ...config, useGpu: e.target.checked })}
                    />
                    <span>Use GPU (Vulkan)</span>
                    <span
                      className={`badge ${
                        vulkanAvailable === true
                          ? "badge-ok"
                          : vulkanAvailable === false
                            ? "badge-warn"
                            : "badge-neutral"
                      }`}
                      title="Whether this build can use Vulkan and the loader is present"
                    >
                      {vulkanAvailable === true
                        ? "Available"
                        : vulkanAvailable === false
                          ? "CPU only"
                          : "Checking…"}
                    </span>
                  </label>
                  <p className="field-hint">
                    Speeds up Whisper when Vulkan works on this machine. If unavailable, transcription still runs on CPU.
                  </p>
                </div>
              </section>

              <section className="settings-section">
                <h2 className="settings-section-title">Appearance</h2>
                <div className="lang-option-row">
                  <label className="lang-radio">
                    <input
                      type="radio"
                      name="themeMode"
                      checked={themeMode === "system"}
                      onChange={() => setThemeMode("system")}
                    />
                    <span>System</span>
                  </label>
                  <label className="lang-radio">
                    <input
                      type="radio"
                      name="themeMode"
                      checked={themeMode === "light"}
                      onChange={() => setThemeMode("light")}
                    />
                    <span>Light</span>
                  </label>
                  <label className="lang-radio">
                    <input
                      type="radio"
                      name="themeMode"
                      checked={themeMode === "dark"}
                      onChange={() => setThemeMode("dark")}
                    />
                    <span>Dark</span>
                  </label>
                </div>
              </section>

              {saveError ? (
                <p style={{ margin: 0, fontSize: 12, color: "var(--status-error)" }}>{saveError}</p>
              ) : null}
              <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
                <button
                  type="button"
                  className="btn-primary"
                  disabled={!storeReady || saveState !== "idle"}
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
                <button
                  type="button"
                  className="btn-secondary"
                  title="Restore default settings (saved podcast feeds are kept)"
                  onClick={() =>
                    // Recent feeds are history, not a setting — resetting the form
                    // must not throw them away.
                    setConfig((prev) => ({
                      ...defaultConfig(),
                      podcastRecents: prev.podcastRecents,
                      podcastOutputDir: prev.podcastOutputDir,
                    }))
                  }
                >
                  Reset defaults
                </button>
              </div>
          </>
        </Modal>
      ) : null}

      {aboutOpen ? (
        <Modal title="About VoxMD" onClose={() => setAboutOpen(false)} panelClassName="about-dialog">
          <>
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
          </>
        </Modal>
      ) : null}
    </div>
  );
}
