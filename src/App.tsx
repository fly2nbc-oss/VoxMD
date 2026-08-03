import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { FileAudio2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { AboutDialog } from "./components/AboutDialog";
import { AppToolbar } from "./components/AppToolbar";
import { ErrorPanel } from "./components/ErrorPanel";
import { PodcastDialog } from "./components/PodcastDialog";
import { QueueTable } from "./components/QueueTable";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { StatusBar } from "./components/StatusBar";
import { defaultConfig } from "./defaults";
import { useBatchEvents } from "./hooks/useBatchEvents";
import { useConfigStore } from "./hooks/useConfigStore";
import { useNativeDrop } from "./hooks/useNativeDrop";
import { useTheme } from "./hooks/useTheme";
import { rememberPodcastRecent } from "./lib/configStore";
import { toMsg } from "./lib/jobs";
import { AUDIO_EXTENSIONS, localItem } from "./lib/queue";
import type { AppConfig, EpisodeInfo, PodcastRecent, QueueItem, WhisperModelInfo } from "./types";

export default function App() {
  const [themeMode, setThemeMode] = useTheme();
  const { config, setConfig, persist, revert, ready: storeReady, loadError } = useConfigStore();
  const batch = useBatchEvents();
  const {
    jobs,
    setJobs,
    processing,
    setProcessing,
    cancelling,
    setCancelling,
    overall,
    setOverall,
    statusMsg,
    setStatusMsg,
    errors,
    setErrors,
    modelDownload,
  } = batch;

  const [items, setItems] = useState<QueueItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [aboutVersion, setAboutVersion] = useState("");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [saveError, setSaveError] = useState("");

  const [podcastOpen, setPodcastOpen] = useState(false);
  const [feedUrl, setFeedUrl] = useState("");
  const [podcastDir, setPodcastDir] = useState("");
  const [feedBusy, setFeedBusy] = useState(false);
  const [podcastError, setPodcastError] = useState("");

  /** `null` until the first fetch resolves; everything looks "custom" before
   *  then, which briefly rendered the custom-path field holding a preset name. */
  const [modelInfos, setModelInfos] = useState<WhisperModelInfo[] | null>(null);
  const [clearingCache, setClearingCache] = useState(false);
  const [vulkanAvailable, setVulkanAvailable] = useState<boolean | null>(null);
  const [detectedSystemSummaryLang, setDetectedSystemSummaryLang] = useState("");

  const saveTimerRef = useRef<number | undefined>(undefined);
  const processingRef = useRef(false);

  useEffect(() => {
    processingRef.current = processing;
  }, [processing]);

  useEffect(() => {
    if (loadError) setStatusMsg(`Settings could not be loaded (${loadError}). Using defaults.`);
  }, [loadError, setStatusMsg]);

  useEffect(() => () => window.clearTimeout(saveTimerRef.current), []);

  useEffect(() => {
    void invoke<{ available: boolean }>("vulkan_status")
      .then((s) => setVulkanAvailable(s.available))
      .catch(() => setVulkanAvailable(null));
  }, []);

  const refreshModelInfos = useCallback(async () => {
    try {
      setModelInfos(await invoke<WhisperModelInfo[]>("list_whisper_models"));
    } catch (e) {
      setModelInfos([]);
      setStatusMsg(`Whisper model list unavailable: ${toMsg(e)}`);
    }
  }, [setStatusMsg]);

  // Refreshed on open so the cached (✓) markers reflect reality. The guard stops
  // a slow response from writing state after the drawer has been closed again.
  useEffect(() => {
    if (!settingsOpen) return;
    let cancelled = false;

    void (async () => {
      const [models, lang] = await Promise.allSettled([
        invoke<WhisperModelInfo[]>("list_whisper_models"),
        invoke<string>("system_summary_language"),
      ]);
      if (cancelled) return;

      if (models.status === "fulfilled") {
        setModelInfos(models.value);
      } else {
        setModelInfos([]);
        setStatusMsg(`Whisper model list unavailable: ${toMsg(models.reason)}`);
      }
      setDetectedSystemSummaryLang(lang.status === "fulfilled" ? lang.value : "");
    })();

    return () => {
      cancelled = true;
    };
  }, [settingsOpen, setStatusMsg]);

  useEffect(() => {
    if (!aboutOpen) return;
    void getVersion()
      .then(setAboutVersion)
      .catch(() => setAboutVersion("—"));
  }, [aboutOpen]);

  /** Append new items (deduplicated by id) and queue rows for them. */
  const addItems = useCallback(
    (added: QueueItem[]) => {
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
      // A finished batch's tally no longer describes the queue.
      setOverall((cur) => (processingRef.current ? cur : null));
    },
    [setJobs, setOverall],
  );

  const dragActive = useNativeDrop(addItems, setStatusMsg);

  const pickFiles = async () => {
    try {
      const sel = await open({
        title: "Audio files",
        multiple: true,
        filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
      });
      if (!sel) return;
      const list = Array.isArray(sel) ? sel : [sel];
      addItems(list.map(localItem));
      setStatusMsg(`${list.length} file(s) added.`);
    } catch (e) {
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
      // Updater form: `config` here predates the feed request above.
      await persist((prev) => rememberPodcastRecent(prev, url, dir, episodes[0]?.feedTitle));
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

  const removePodcastRecent = (url: string) => {
    void persist((prev) => ({
      ...prev,
      podcastRecents: prev.podcastRecents.filter((r) => r.feedUrl !== url),
    }));
  };

  const toggleSelect = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleSelectAll = (checked: boolean) =>
    setSelected(checked ? new Set(items.map((i) => i.id)) : new Set());

  const removeSelected = () => {
    const count = selected.size;
    if (count === 0) return;
    setItems((prev) => prev.filter((i) => !selected.has(i.id)));
    setJobs((prev) =>
      Object.fromEntries(Object.entries(prev).filter(([k]) => !selected.has(k))),
    );
    setSelected(new Set());
    setOverall(null);
    setStatusMsg(`${count} entr${count === 1 ? "y" : "ies"} removed from the list.`);
  };

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

  const hasApiKey = config.apiKey.trim() !== "";
  // The summary only runs with a key; without one the transcript must carry the output.
  const outputInvalid = !(config.includeSummary && hasApiKey) && !config.includeTranscript;

  const start = async () => {
    if (!storeReady) {
      setStatusMsg("Settings are still loading…");
      return;
    }
    if (items.length === 0) {
      setStatusMsg("No entries in queue.");
      return;
    }
    // Selection scopes the batch; with nothing checked, process the whole queue.
    const toProcess = selected.size > 0 ? items.filter((i) => selected.has(i.id)) : items;
    if (toProcess.length === 0) {
      setStatusMsg("No entries selected.");
      return;
    }

    // Seeded before invoking: the backend starts emitting immediately, and doing
    // this afterwards discarded events that had already arrived.
    setProcessing(true);
    setCancelling(false);
    setSelected(new Set());
    setErrors([]);
    setStatusMsg("");
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
    void persist(next);
  };

  const clearCache = async () => {
    setClearingCache(true);
    try {
      await invoke("clear_whisper_cache");
      await refreshModelInfos();
    } catch (e) {
      // Shown in the drawer: the footer status line sits behind the overlay.
      setSaveError(`Could not clear the cache: ${toMsg(e)}`);
    } finally {
      setClearingCache(false);
    }
  };

  const pickModelFile = async () => {
    try {
      const file = await open({
        title: "Whisper model file",
        multiple: false,
        filters: [{ name: "Whisper model", extensions: ["bin", "gguf"] }],
      });
      if (typeof file === "string" && file) setConfig({ ...config, whisperModel: file });
    } catch (e) {
      setSaveError(`Could not open the file picker: ${toMsg(e)}`);
    }
  };

  const closeSettings = () => {
    // Discard unsaved edits; they used to stay in the live config, apply to the
    // next run, and get written by the next toolbar toggle.
    revert();
    setSettingsOpen(false);
    setSaveState("idle");
    setSaveError("");
  };

  const handleSaveSettings = async () => {
    setSaveState("saving");
    setSaveError("");
    try {
      await persist(config);
      setSaveState("saved");
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

  return (
    <div className="app-shell">
      <AppToolbar
        config={config}
        storeReady={storeReady}
        processing={processing}
        cancelling={cancelling}
        itemCount={items.length}
        selectedCount={selected.size}
        outputInvalid={outputInvalid}
        onPickFiles={pickFiles}
        onOpenPodcast={openPodcast}
        onRemoveSelected={removeSelected}
        onStart={() => void start()}
        onCancel={() => void cancelProcessing()}
        onToggleMd={toggleMdOutput}
        onToggleDeleteSource={() =>
          void persist((prev) => ({
            ...prev,
            deleteSourceAfterSuccess: !prev.deleteSourceAfterSuccess,
          }))
        }
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenAbout={() => setAboutOpen(true)}
      />

      {dragActive ? (
        <div className="drop-overlay" aria-hidden>
          <div className="drop-overlay-inner">
            <FileAudio2 size={40} aria-hidden />
            <p>Drop audio files to add them to the queue</p>
          </div>
        </div>
      ) : null}

      <ErrorPanel errors={errors} onDismiss={() => setErrors([])} />

      <main className="content">
        <QueueTable
          items={items}
          jobs={jobs}
          selected={selected}
          processing={processing}
          onToggle={toggleSelect}
          onToggleAll={toggleSelectAll}
          onOpenResult={(p) => void openResult(p)}
          onRevealResult={(p) => void revealResult(p)}
        />
        <StatusBar
          itemCount={items.length}
          overall={overall}
          modelDownload={modelDownload}
          processing={processing}
          cancelling={cancelling}
          statusMsg={statusMsg}
        />
      </main>

      {podcastOpen ? (
        <PodcastDialog
          feedUrl={feedUrl}
          onFeedUrlChange={setFeedUrl}
          outputDir={podcastDir}
          onOutputDirChange={setPodcastDir}
          onChooseDir={() => void choosePodcastDir()}
          recents={config.podcastRecents}
          onApplyRecent={applyPodcastRecent}
          onRemoveRecent={removePodcastRecent}
          busy={feedBusy}
          error={podcastError}
          onSubmit={() => void addPodcast()}
          onClose={() => setPodcastOpen(false)}
        />
      ) : null}

      {settingsOpen ? (
        <SettingsDrawer
          config={config}
          onConfigChange={setConfig}
          storeReady={storeReady}
          saveState={saveState}
          saveError={saveError}
          onSave={() => void handleSaveSettings()}
          onResetDefaults={() =>
            // Recent feeds are history, not a setting.
            setConfig((prev: AppConfig) => ({
              ...defaultConfig(),
              podcastRecents: prev.podcastRecents,
              podcastOutputDir: prev.podcastOutputDir,
            }))
          }
          onClose={closeSettings}
          modelInfos={modelInfos ?? []}
          modelsLoading={modelInfos === null}
          clearingCache={clearingCache}
          onClearCache={() => void clearCache()}
          onPickModelFile={() => void pickModelFile()}
          detectedSystemSummaryLang={detectedSystemSummaryLang}
          vulkanAvailable={vulkanAvailable}
          themeMode={themeMode}
          onThemeChange={setThemeMode}
        />
      ) : null}

      {aboutOpen ? (
        <AboutDialog version={aboutVersion} onClose={() => setAboutOpen(false)} />
      ) : null}
    </div>
  );
}
