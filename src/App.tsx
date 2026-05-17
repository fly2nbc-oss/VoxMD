import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Store } from "@tauri-apps/plugin-store";
import {
  FileAudio2,
  FolderOpen,
  Loader2,
  Moon,
  Play,
  Settings,
  Sun,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { defaultConfig } from "./defaults";
import type { AppConfig } from "./types";

const STORE_FILE = "voxmd-settings.json";
const CONFIG_KEY = "appConfig";

function mergeConfig(saved: Partial<AppConfig> | null | undefined): AppConfig {
  const base = defaultConfig();
  if (!saved) return base;
  return {
    ...base,
    ...saved,
    apiKey: saved.apiKey?.trim() ?? "",
    apiBaseUrl: saved.apiBaseUrl?.trim() ? saved.apiBaseUrl : base.apiBaseUrl,
    apiModel: saved.apiModel?.trim() ? saved.apiModel : base.apiModel,
  };
}

interface JobProgressPayload {
  path: string;
  displayName: string;
  stage: string;
  whisperPct?: number;
  llmChunk?: [number, number];
  overall?: { completed: number; total: number; pct: number };
  message?: string;
}

interface JobRow extends JobProgressPayload {
  whisperPct?: number;
}

function badgeForStage(stage: string): { className: string; label: string } {
  switch (stage) {
    case "done":    return { className: "badge-ok",      label: "Done"    };
    case "error":   return { className: "badge-error",   label: "Error"   };
    case "skipped": return { className: "badge-warn",    label: "Skipped" };
    case "whisper": return { className: "badge-warn",    label: "Whisper" };
    case "llm":     return { className: "badge-neutral", label: "LLM"     };
    case "queued":  return { className: "badge-neutral", label: "Wait"    };
    default:        return { className: "badge-neutral", label: stage     };
  }
}

function detailsForRow(row: JobRow): string {
  switch (row.stage) {
    case "queued":
      return "Waiting in queue…";
    case "whisper":
      return "Transcribing…";
    case "llm":
      if (row.llmChunk) {
        return `Speakers · chunk ${row.llmChunk[0]} / ${row.llmChunk[1]}`;
      }
      return row.message ?? "";
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
  const [paths, setPaths] = useState<string[]>([]);
  const [jobs, setJobs] = useState<Record<string, JobRow>>({});
  const [processing, setProcessing] = useState(false);
  const [overall, setOverall] = useState<{ completed: number; total: number } | null>(null);
  const [statusMsg, setStatusMsg] = useState("");
  const [modelDownload, setModelDownload] = useState<{ pct: number; model: string } | null>(null);
  const [modelInfos, setModelInfos] = useState<Array<{ name: string; sizeHint: string; cached: boolean }>>([]);
  const [clearingCache, setClearingCache] = useState(false);
  const settingsWasOpen = useRef(false);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    (async () => {
      try {
        const store = await Store.load(STORE_FILE, { autoSave: true, defaults: {} });
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
    } catch {
      // silently ignore if command unavailable
    }
  }, []);

  useEffect(() => {
    if (settingsOpen && !settingsWasOpen.current) {
      refreshModelInfos();
    }
    settingsWasOpen.current = settingsOpen;
  }, [settingsOpen, refreshModelInfos]);

  const clearCache = useCallback(async () => {
    setClearingCache(true);
    try {
      await invoke("clear_whisper_cache");
      await refreshModelInfos();
    } catch (e) {
      setStatusMsg(String(e));
    } finally {
      setClearingCache(false);
    }
  }, [refreshModelInfos]);

  const saveConfig = useCallback(async (c: AppConfig) => {
    const store = await Store.load(STORE_FILE, { autoSave: true, defaults: {} });
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
          [p.path]: { ...p, whisperPct: p.whisperPct ?? prev[p.path]?.whisperPct },
        }));
        if (p.overall) {
          setOverall({ completed: p.overall.completed, total: p.overall.total });
        }
        if (p.message && (p.stage === "done" || p.stage === "error" || p.stage === "skipped")) {
          setStatusMsg(p.message);
        }
      });
      unlistenDone = await listen<{ total: number }>("batch_complete", () => {
        setProcessing(false);
        setModelDownload(null);
        setStatusMsg("Batch complete.");
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

  const initJobsFromPaths = useCallback((ps: string[]) => {
    const next: Record<string, JobRow> = {};
    for (const p of ps) {
      const name = p.split(/[/\\]/).pop() ?? p;
      next[p] = {
        path: p,
        displayName: name,
        stage: "queued",
      };
    }
    setJobs(next);
    setOverall(null);
  }, []);

  const pickFolder = async () => {
    const dir = await open({
      title: "Folder with audio files",
      directory: true,
      recursive: true,
    });
    if (typeof dir !== "string" || !dir) return;
    const list = await invoke<string[]>("collect_audio_in_directory", { dir });
    setPaths(list);
    initJobsFromPaths(list);
    setStatusMsg(`${list.length} file(s) from folder.`);
  };

  const pickFiles = async () => {
    const sel = await open({
      title: "Audio files",
      multiple: true,
      filters: [
        {
          name: "Audio",
          extensions: ["mp3", "m4a", "mp4", "wav", "ogg", "flac", "webm", "opus"],
        },
      ],
    });
    if (!sel) return;
    const list = Array.isArray(sel) ? sel : [sel];
    setPaths(list);
    initJobsFromPaths(list);
    setStatusMsg(`${list.length} file(s) selected.`);
  };

  const start = async () => {
    if (!paths.length) {
      setStatusMsg("No files in queue.");
      return;
    }
    try {
      await invoke("start_transcription", { paths, config });
      setProcessing(true);
      setStatusMsg("Processing started…");
      setOverall({ completed: 0, total: paths.length });
    } catch (e) {
      setStatusMsg(String(e));
    }
  };

  const stopUiHint =
    "The running backend job cannot be cancelled yet; progress is shown until the current batch finishes.";

  const rows = useMemo(() => paths.map((p) => jobs[p] ?? { path: p, displayName: p, stage: "queued" }), [paths, jobs]);

  const overallPct = overall && overall.total > 0 ? (overall.completed / overall.total) * 100 : 0;

  return (
    <div className="app-shell">
      <header className="app-bar">
        <h1 className="app-bar-title">VoxMD</h1>
        <div className="app-bar-actions">
          <button type="button" className="btn-secondary btn-sm" onClick={pickFolder} title="Folder with audio files">
            <FolderOpen size={14} aria-hidden />
            <span>Folder</span>
          </button>
          <button type="button" className="btn-secondary btn-sm" onClick={pickFiles} title="Select audio files">
            <FileAudio2 size={14} aria-hidden />
            <span>Files</span>
          </button>
          <button
            type="button"
            className="btn-primary btn-sm"
            disabled={processing || !paths.length}
            onClick={start}
            title="Start processing"
          >
            <Play size={14} aria-hidden />
            <span>Start</span>
          </button>
          {processing ? (
            <span className="badge badge-neutral" title={stopUiHint}>
              <Loader2 size={12} className="icon" style={{ animation: "spin 1s linear infinite" }} />
              Running
            </span>
          ) : null}
        </div>
        <div className="app-bar-end">
          <button
            type="button"
            className="icon-btn"
            title={theme === "dark" ? "Light mode" : "Dark mode"}
            aria-label="Theme"
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
          >
            {theme === "dark" ? <Sun className="icon" size={16} /> : <Moon className="icon" size={16} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Settings"
            aria-label="Settings"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className="icon" size={16} />
          </button>
        </div>
      </header>

      <main className="content">
        {paths.length === 0 ? (
          <p className="empty-title">Choose a folder or audio files, then press Start.</p>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th style={{ width: "40%" }}>File</th>
                  <th style={{ width: "80px" }}>Status</th>
                  <th>Details</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const b = badgeForStage(row.stage);
                  const details = detailsForRow(row);
                  return (
                    <tr key={row.path}>
                      <td className="mono">{row.displayName}</td>
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
                : paths.length
                  ? `${paths.length} queued`
                  : "Empty"}
          </span>
          <div className="progress-track" title={modelDownload ? `Downloading model: ${modelDownload.pct}%` : "Overall progress"}>
            <div
              className="progress-bar"
              style={{ width: `${Math.min(100, modelDownload ? modelDownload.pct : overallPct)}%` }}
            />
          </div>
          <span className="mono" style={{ maxWidth: "45%", textAlign: "right" }}>
            {modelDownload ? `${modelDownload.pct}%` : statusMsg}
          </span>
        </footer>
      </main>

      {settingsOpen ? (
        <div className="drawer-overlay" role="presentation" onMouseDown={() => setSettingsOpen(false)}>
          <aside className="drawer" onMouseDown={(ev) => ev.stopPropagation()}>
            <div className="drawer-header">
              <strong>Settings</strong>
              <button type="button" className="icon-btn" title="Close" aria-label="Close" onClick={() => setSettingsOpen(false)}>
                <X size={18} />
              </button>
            </div>
            <div className="drawer-body">
              <p style={{ margin: 0, color: "var(--muted)", fontSize: 12 }}>
                Settings are stored locally. API keys never leave this device.
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
                  onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label" htmlFor="apiBase">
                  API base URL
                </label>
                <input
                  id="apiBase"
                  className="input"
                  value={config.apiBaseUrl}
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
                  onChange={(e) => setConfig({ ...config, apiModel: e.target.value })}
                />
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div>
                  <label className="field-label" htmlFor="temp">
                    Temperature
                  </label>
                  <input
                    id="temp"
                    className="input"
                    type="number"
                    step="0.1"
                    value={config.temperature}
                    onChange={(e) => setConfig({ ...config, temperature: Number(e.target.value) })}
                  />
                </div>
                <div>
                  <label className="field-label" htmlFor="mtok">
                    Max tokens
                  </label>
                  <input
                    id="mtok"
                    className="input"
                    type="number"
                    value={config.maxTokens}
                    onChange={(e) => setConfig({ ...config, maxTokens: Number(e.target.value) })}
                  />
                </div>
              </div>
              <div>
                <label className="field-label" htmlFor="chunks">
                  Transcript chunk (characters)
                </label>
                <input
                  id="chunks"
                  className="input"
                  type="number"
                  value={config.transcriptChunkChars}
                  onChange={(e) => setConfig({ ...config, transcriptChunkChars: Number(e.target.value) })}
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
                <label className="field-label" htmlFor="wthreads">
                  Whisper CPU threads (empty = auto)
                </label>
                <input
                  id="wthreads"
                  className="input"
                  type="number"
                  placeholder="auto"
                  value={config.whisperThreads ?? ""}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      whisperThreads: e.target.value === "" ? null : Number(e.target.value),
                    })
                  }
                />
              </div>
              <div>
                <label className="field-label" htmlFor="lang">
                  Language (ISO, e.g. de)
                </label>
                <input
                  id="lang"
                  className="input"
                  value={config.language}
                  onChange={(e) => setConfig({ ...config, language: e.target.value })}
                />
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={config.useGpu}
                  onChange={(e) => setConfig({ ...config, useGpu: e.target.checked })}
                />
                <span>Use GPU (only if VoxMD was built with the gpu-vulkan feature)</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={config.deleteSourceAfterSuccess}
                  onChange={(e) => setConfig({ ...config, deleteSourceAfterSuccess: e.target.checked })}
                />
                <span>Delete source audio after successful MD export</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={config.whisperVerbose}
                  onChange={(e) => setConfig({ ...config, whisperVerbose: e.target.checked })}
                />
                <span>Whisper verbose output (WHISPER_CPP_VERBOSE)</span>
              </label>

              <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
                <button type="button" className="btn-primary" disabled={!storeReady} onClick={() => saveConfig(config)}>
                  Save
                </button>
                <button type="button" className="btn-secondary" onClick={() => setConfig(defaultConfig())}>
                  Reset defaults
                </button>
              </div>
            </div>
          </aside>
        </div>
      ) : null}

      <style>{`
        @keyframes spin { to { transform: rotate(360deg); } }
      `}</style>
    </div>
  );
}