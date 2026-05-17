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
import { useCallback, useEffect, useMemo, useState } from "react";

const STORE_FILE = "voxmd-settings.json";
const CONFIG_KEY = "appConfig";

export interface AppConfig {
  apiKey: string;
  apiBaseUrl: string;
  apiModel: string;
  temperature: number;
  maxTokens: number;
  transcriptChunkChars: number;
  whisperModelPath: string;
  whisperThreads: number | null;
  language: string;
  useGpu: boolean;
  deleteSourceAfterSuccess: boolean;
}

const defaultConfig = (): AppConfig => ({
  apiKey: "",
  apiBaseUrl: "https://api.deepseek.com/v1",
  apiModel: "deepseek-chat",
  temperature: 0.3,
  maxTokens: 8192,
  transcriptChunkChars: 32768,
  whisperModelPath: "",
  whisperThreads: null,
  language: "de",
  useGpu: true,
  deleteSourceAfterSuccess: false,
});

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
    case "done":
      return { className: "badge-ok", label: "Fertig" };
    case "error":
      return { className: "badge-error", label: "Fehler" };
    case "skipped":
      return { className: "badge-warn", label: "Übersprungen" };
    case "whisper":
      return { className: "badge-warn", label: "Whisper" };
    case "llm":
      return { className: "badge-neutral", label: "LLM" };
    default:
      return { className: "badge-neutral", label: stage };
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

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    (async () => {
      try {
        const store = await Store.load(STORE_FILE, { autoSave: true, defaults: {} });
        const saved = await store.get<AppConfig>(CONFIG_KEY);
        if (saved) {
          setConfig({ ...defaultConfig(), ...saved });
        }
        setStoreReady(true);
      } catch {
        setStoreReady(true);
      }
    })();
  }, []);

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
        if (p.message) {
          setStatusMsg(p.message);
        }
      });
      unlistenDone = await listen<{ total: number }>("batch_complete", () => {
        setProcessing(false);
        setStatusMsg("Batch abgeschlossen.");
      });
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
      title: "Ordner mit Audio-Dateien",
      directory: true,
      recursive: true,
    });
    if (typeof dir !== "string" || !dir) return;
    const list = await invoke<string[]>("collect_audio_in_directory", { dir });
    setPaths(list);
    initJobsFromPaths(list);
    setStatusMsg(`${list.length} Datei(en) aus Ordner.`);
  };

  const pickFiles = async () => {
    const sel = await open({
      title: "Audio-Dateien",
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
    setStatusMsg(`${list.length} Datei(en) ausgewählt.`);
  };

  const start = async () => {
    if (!paths.length) {
      setStatusMsg("Keine Dateien in der Warteschlange.");
      return;
    }
    try {
      await invoke("start_transcription", { paths, config });
      setProcessing(true);
      setStatusMsg("Verarbeitung gestartet …");
      setOverall({ completed: 0, total: paths.length });
    } catch (e) {
      setStatusMsg(String(e));
    }
  };

  const stopUiHint =
    "Die laufende Backend-Verarbeitung kann momentan nicht hart abgebrochen werden; die App zeigt den Fortschritt bis zum Ende des aktuellen Jobs.";

  const rows = useMemo(() => paths.map((p) => jobs[p] ?? { path: p, displayName: p, stage: "queued" }), [paths, jobs]);

  const overallPct = overall && overall.total > 0 ? (overall.completed / overall.total) * 100 : 0;

  return (
    <div className="app-shell">
      <header className="topbar">
        <h1>VoxMD</h1>
        <div className="topbar-actions">
          <button
            type="button"
            className="icon-btn"
            title={theme === "dark" ? "Hellmodus" : "Dunkelmodus"}
            aria-label="Theme"
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
          >
            {theme === "dark" ? <Sun className="icon" size={20} /> : <Moon className="icon" size={20} />}
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Einstellungen"
            aria-label="Einstellungen"
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className="icon" size={20} />
          </button>
        </div>
      </header>

      <div className="toolbar">
        <button type="button" className="btn-secondary" onClick={pickFolder}>
          <FolderOpen size={16} style={{ marginRight: 8 }} />
          Ordner
        </button>
        <button type="button" className="btn-secondary" onClick={pickFiles}>
          <FileAudio2 size={16} style={{ marginRight: 8 }} />
          Dateien
        </button>
        <button type="button" className="btn-primary" disabled={processing || !paths.length} onClick={start}>
          <Play size={16} style={{ marginRight: 8 }} />
          Start
        </button>
        {processing ? (
          <span className="badge badge-neutral" title={stopUiHint}>
            <Loader2 size={14} className="icon" style={{ animation: "spin 1s linear infinite" }} />
            Läuft
          </span>
        ) : null}
      </div>

      <main className="content">
        {paths.length === 0 ? (
          <p className="empty-title">Ordner oder Audio-Dateien wählen, dann Start.</p>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Datei</th>
                  <th>Status</th>
                  <th>Whisper</th>
                  <th>LLM</th>
                  <th>Info</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const b = badgeForStage(row.stage);
                  const llmTxt =
                    row.llmChunk && row.stage === "llm"
                      ? `Abschnitt ${row.llmChunk[0]} / ${row.llmChunk[1]}`
                      : "–";
                  const whPct = row.whisperPct;
                  const whTxt =
                    row.stage === "whisper" && whPct != null ? `${whPct} %` : row.stage === "whisper" ? "…" : "–";
                  return (
                    <tr key={row.path}>
                      <td className="mono">{row.displayName}</td>
                      <td>
                        <span className={`badge ${b.className}`}>{b.label}</span>
                      </td>
                      <td>{whTxt}</td>
                      <td>{llmTxt}</td>
                      <td className="mono" style={{ color: "var(--muted)", maxWidth: 360 }}>
                        {row.message ?? ""}
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
            {overall
              ? `Gesamt: ${overall.completed} / ${overall.total} fertig (MD)`
              : paths.length
                ? `${paths.length} in Warteschlange`
                : "Leer"}
          </span>
          <div className="progress-track" title="Fortschritt über alle erfolgreich abgeschlossenen Dateien">
            <div className="progress-bar" style={{ width: `${Math.min(100, overallPct)}%` }} />
          </div>
          <span className="mono" style={{ maxWidth: "45%", textAlign: "right" }}>
            {statusMsg}
          </span>
        </footer>
      </main>

      {settingsOpen ? (
        <div className="drawer-overlay" role="presentation" onMouseDown={() => setSettingsOpen(false)}>
          <aside className="drawer" onMouseDown={(ev) => ev.stopPropagation()}>
            <div className="drawer-header">
              <strong>Einstellungen</strong>
              <button type="button" className="icon-btn" title="Schließen" aria-label="Schließen" onClick={() => setSettingsOpen(false)}>
                <X size={20} />
              </button>
            </div>
            <div className="drawer-body">
              <p style={{ margin: 0, color: "var(--muted)", fontSize: 12 }}>
                Konfiguration wird mit dem Store-Plugin gespeichert. API-Keys bleiben lokal.
              </p>

              <div>
                <label className="field-label" htmlFor="apiKey">
                  API-Key
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
                  API Base URL
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
                  Modell
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
                    Temperatur
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
                    Max Tokens
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
                  Transkript Chunk (Zeichen)
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
                <label className="field-label" htmlFor="wpath">
                  Whisper-Modell (Pfad zur GGUF/GGML-Datei)
                </label>
                <input
                  id="wpath"
                  className="input"
                  value={config.whisperModelPath}
                  onChange={(e) => setConfig({ ...config, whisperModelPath: e.target.value })}
                />
              </div>
              <div>
                <label className="field-label" htmlFor="wthreads">
                  Whisper CPU-Threads (leer = auto)
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
                  Sprache (ISO, z. B. de)
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
                <span>GPU nutzen (nur wenn VoxMD mit Feature „gpu-vulkan“ gebaut wurde)</span>
              </label>
              <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={config.deleteSourceAfterSuccess}
                  onChange={(e) => setConfig({ ...config, deleteSourceAfterSuccess: e.target.checked })}
                />
                <span>Quell-Audio nach erfolgreicher MD-Datei löschen</span>
              </label>

              <div style={{ display: "flex", gap: 8, marginTop: "auto" }}>
                <button type="button" className="btn-primary" disabled={!storeReady} onClick={() => saveConfig(config)}>
                  Speichern
                </button>
                <button type="button" className="btn-secondary" onClick={() => setConfig(defaultConfig())}>
                  Zurücksetzen
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
