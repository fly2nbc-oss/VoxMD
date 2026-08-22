import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { FileAudio2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { AboutDialog } from "./components/AboutDialog";
import { AppToolbar } from "./components/AppToolbar";
import { DictationView } from "./components/DictationView";
import { ErrorPanel } from "./components/ErrorPanel";
import { PodcastDialog } from "./components/PodcastDialog";
import { QueueTable } from "./components/QueueTable";
import { SettingsDrawer } from "./components/SettingsDrawer";
import { StatusBar } from "./components/StatusBar";
import { defaultConfig } from "./defaults";
import { useBatchEvents } from "./hooks/useBatchEvents";
import { useConfigStore } from "./hooks/useConfigStore";
import { useDictationEvents } from "./hooks/useDictationEvents";
import { useHotkeys } from "./hooks/useHotkeys";
import { useNativeDrop } from "./hooks/useNativeDrop";
import { useTheme } from "./hooks/useTheme";
import { I18nContext, useI18nValue } from "./i18n/I18nProvider";
import { rememberPodcastRecent } from "./lib/configStore";
import { summaryWouldRun } from "./lib/llmProviders";
import { toMsg } from "./lib/jobs";
import { itemsToPersist, parseSavedQueue } from "./lib/queuePersist";
import { AUDIO_EXTENSIONS, localItem } from "./lib/queue";
import type {
  AppConfig,
  AppMode,
  EpisodeInfo,
  PodcastRecent,
  QueueItem,
  WhisperModelInfo,
} from "./types";

export default function App() {
  const [themeMode, setThemeMode] = useTheme();
  const {
    config,
    setConfig,
    persist,
    revert,
    loadQueue,
    saveQueue,
    ready: storeReady,
    loadError,
  } = useConfigStore();
  // Before `useBatchEvents`: that hook needs `t`, and App is the component that
  // provides the context, so it cannot read it back through `useT`.
  const i18n = useI18nValue(config.uiLanguage);
  const { t, tn } = i18n;

  const batch = useBatchEvents(t);
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

  const dictation = useDictationEvents(setStatusMsg);
  const dictating = dictation.running;

  const [items, setItems] = useState<QueueItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [mode, setMode] = useState<AppMode>("queue");
  const [queueHydrated, setQueueHydrated] = useState(false);

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [aboutVersion, setAboutVersion] = useState("");
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">(
    "idle",
  );
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
  const [detectedSystemSummaryLang, setDetectedSystemSummaryLang] =
    useState("");

  const saveTimerRef = useRef<number | undefined>(undefined);
  const processingRef = useRef(false);
  const dictatingRef = useRef(false);
  /** Mirrors `items` so `addItems` can dedupe without a state updater. */
  const itemsRef = useRef<QueueItem[]>([]);

  useEffect(() => {
    processingRef.current = processing;
  }, [processing]);

  useEffect(() => {
    dictatingRef.current = dictating;
  }, [dictating]);

  useEffect(() => {
    if (loadError)
      setStatusMsg(t("msg.settingsLoadFailed", { error: loadError }));
  }, [loadError, setStatusMsg, t]);

  useEffect(() => () => window.clearTimeout(saveTimerRef.current), []);

  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  useEffect(() => {
    void invoke<{ available: boolean }>("vulkan_status")
      .then((s) => setVulkanAvailable(s.available))
      .catch(() => setVulkanAvailable(null));
  }, []);

  useEffect(() => {
    if (!storeReady) return;
    let cancelled = false;
    void (async () => {
      let loaded = false;
      try {
        const restored = parseSavedQueue(await loadQueue());
        if (cancelled) return;
        if (restored.length > 0) {
          itemsRef.current = restored;
          setItems(restored);
          setJobs(
            Object.fromEntries(
              restored.map((item) => [
                item.id,
                {
                  path: item.id,
                  displayName: item.displayName,
                  stage: "queued",
                },
              ]),
            ),
          );
        }
        loaded = true;
      } catch (e) {
        if (!cancelled)
          setStatusMsg(t("msg.queueRestoreFailed", { error: toMsg(e) }));
      } finally {
        if (!cancelled && loaded) setQueueHydrated(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [storeReady, loadQueue, setJobs, setStatusMsg, t]);

  useEffect(() => {
    if (!queueHydrated) return;
    const timer = window.setTimeout(() => {
      void saveQueue(itemsToPersist(items, jobs));
    }, 500);
    return () => window.clearTimeout(timer);
  }, [items, jobs, queueHydrated, saveQueue]);

  const refreshModelInfos = useCallback(async () => {
    try {
      setModelInfos(await invoke<WhisperModelInfo[]>("list_whisper_models"));
    } catch (e) {
      setModelInfos([]);
      setStatusMsg(t("msg.modelListUnavailable", { error: toMsg(e) }));
    }
  }, [setStatusMsg, t]);

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
        setStatusMsg(
          t("msg.modelListUnavailable", { error: toMsg(models.reason) }),
        );
      }
      setDetectedSystemSummaryLang(
        lang.status === "fulfilled" ? lang.value : "",
      );
    })();

    return () => {
      cancelled = true;
    };
  }, [settingsOpen, setStatusMsg, t]);

  useEffect(() => {
    if (!aboutOpen) return;
    void getVersion()
      .then(setAboutVersion)
      .catch(() => setAboutVersion("—"));
  }, [aboutOpen]);

  /** Append new items (deduplicated by id) and queue rows for them.
   *
   *  Dedupe and the IPC call run outside the state updaters: React may invoke an
   *  updater more than once (StrictMode does so in development), which sent the
   *  same file to `append_to_batch` twice. */
  const addItems = useCallback(
    (added: QueueItem[]) => {
      const seen = new Set(itemsRef.current.map((i) => i.id));
      const fresh: QueueItem[] = [];
      for (const item of added) {
        if (seen.has(item.id)) continue;
        seen.add(item.id);
        fresh.push(item);
      }
      if (fresh.length === 0) return;

      itemsRef.current = [...itemsRef.current, ...fresh];
      setItems(itemsRef.current);
      setJobs((prevJobs) => {
        const next = { ...prevJobs };
        for (const item of fresh) {
          next[item.id] = {
            path: item.id,
            displayName: item.displayName,
            stage: "queued",
          };
        }
        return next;
      });
      if (processingRef.current) {
        void invoke("append_to_batch", { items: fresh }).catch((e) =>
          setStatusMsg(toMsg(e)),
        );
      }
      // A finished batch's tally no longer describes the queue.
      setOverall((cur) => (processingRef.current ? cur : null));
    },
    [setJobs, setOverall, setStatusMsg],
  );

  const dragActive = useNativeDrop(addItems, setStatusMsg, t);

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
      setStatusMsg(
        processingRef.current
          ? t("msg.filesAddedRunning", { count: list.length })
          : t("msg.filesAdded", { count: list.length }),
      );
    } catch (e) {
      setStatusMsg(t("msg.filePickerFailed", { error: toMsg(e) }));
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
      setPodcastError(t("msg.folderPickerFailed", { error: toMsg(e) }));
    }
  };

  const addPodcast = async () => {
    setFeedBusy(true);
    setPodcastError("");
    try {
      const url = feedUrl.trim();
      const dir = podcastDir.trim();
      const episodes = await invoke<EpisodeInfo[]>("fetch_podcast_feed", {
        url,
      });
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
      setStatusMsg(t("msg.episodesAdded", { count: episodes.length }));
      // Updater form: `config` here predates the feed request above.
      await persist((prev) =>
        rememberPodcastRecent(prev, url, dir, episodes[0]?.feedTitle),
      );
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
    itemsRef.current = itemsRef.current.filter((i) => !selected.has(i.id));
    setItems(itemsRef.current);
    setJobs((prev) =>
      Object.fromEntries(
        Object.entries(prev).filter(([k]) => !selected.has(k)),
      ),
    );
    setSelected(new Set());
    setOverall(null);
    setStatusMsg(tn("msg.entriesRemovedOne", "msg.entriesRemovedMany", count));
  };

  const openResult = async (path: string) => {
    try {
      await openPath(path);
    } catch {
      try {
        await revealItemInDir(path);
      } catch (e) {
        setStatusMsg(t("msg.openFailed", { path, error: toMsg(e) }));
      }
    }
  };

  const revealResult = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (e) {
      setStatusMsg(t("msg.openFolderFailed", { error: toMsg(e) }));
    }
  };

  // The summary needs a key, or a model server on this machine; without either
  // the transcript must carry the output. Same rule as `summary_enabled` in Rust.
  const outputInvalid = !summaryWouldRun(config) && !config.includeTranscript;

  const start = async () => {
    if (!storeReady) {
      setStatusMsg(t("msg.settingsLoading"));
      return;
    }
    if (dictatingRef.current) {
      setStatusMsg(t("msg.stopDictationFirst"));
      return;
    }
    if (items.length === 0) {
      setStatusMsg(t("msg.emptyQueue"));
      return;
    }
    // Selection scopes the batch; with nothing checked, process the whole queue.
    const toProcess =
      selected.size > 0 ? items.filter((i) => selected.has(i.id)) : items;
    if (toProcess.length === 0) {
      setStatusMsg(t("msg.noSelection"));
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
        next[item.id] = {
          path: item.id,
          displayName: item.displayName,
          stage: "queued",
        };
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
      setStatusMsg(t("msg.cancelRequested"));
    } catch (e) {
      setCancelling(false);
      setStatusMsg(toMsg(e));
    }
  };

  const startDictation = async () => {
    if (processingRef.current) {
      setStatusMsg(t("msg.stopBatchFirst"));
      return;
    }
    try {
      // `dictating` flips on the `dictation_status` event, not here: the backend
      // is the owner, and guessing locally is what let the two drift apart.
      await invoke("start_dictation", { config });
    } catch (e) {
      setStatusMsg(toMsg(e));
    }
  };

  const stopDictation = () => {
    invoke("stop_dictation").catch((e) => setStatusMsg(toMsg(e)));
  };

  const switchMode = (next: AppMode) => {
    if (next === "dictation" && processingRef.current) {
      setStatusMsg(t("msg.stopBatchFirst"));
      return;
    }
    if (next === "queue" && dictatingRef.current) {
      stopDictation();
    }
    setMode(next);
  };

  const toggleMdOutput = (
    key: "includeMeta" | "includeSummary" | "includeTranscript",
  ) => {
    const next = { ...config, [key]: !config[key] };
    if (key !== "includeMeta") {
      if (!summaryWouldRun(next) && !next.includeTranscript) {
        setStatusMsg(
          next.includeSummary
            ? t("msg.needKeyOrTranscript")
            : t("msg.needSummaryOrTranscript"),
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
      setSaveError(t("msg.clearCacheFailed", { error: toMsg(e) }));
    } finally {
      setClearingCache(false);
    }
  };

  const pickModelFile = async (field: "whisperModel" | "dictationModel") => {
    try {
      const file = await open({
        title: "Whisper model file",
        multiple: false,
        filters: [{ name: "Whisper model", extensions: ["bin", "gguf"] }],
      });
      if (typeof file === "string" && file)
        setConfig({ ...config, [field]: file });
    } catch (e) {
      setSaveError(t("msg.filePickerFailed", { error: toMsg(e) }));
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

  const dialogOpen = settingsOpen || aboutOpen || podcastOpen;

  useHotkeys(
    {
      onStartOrToggleRecord: () => {
        if (mode === "dictation") {
          if (dictatingRef.current) stopDictation();
          else void startDictation();
          return;
        }
        if (!processingRef.current) void start();
      },
      onCancelOrStop: () => {
        if (dictatingRef.current) {
          stopDictation();
          return;
        }
        if (processingRef.current) void cancelProcessing();
      },
      onPickFiles: () => {
        if (dictatingRef.current) {
          setStatusMsg(t("msg.stopDictationBeforeFiles"));
          return;
        }
        setMode("queue");
        void pickFiles();
      },
      onOpenSettings: () => setSettingsOpen(true),
      onQueueMode: () => switchMode("queue"),
      onDictationMode: () => switchMode("dictation"),
    },
    !dialogOpen,
  );

  return (
    <I18nContext.Provider value={i18n}>
      <div className="app-shell">
        <AppToolbar
          config={config}
          storeReady={storeReady}
          mode={mode}
          processing={processing}
          dictating={dictating}
          cancelling={cancelling}
          itemCount={items.length}
          selectedCount={selected.size}
          outputInvalid={outputInvalid}
          onModeChange={switchMode}
          onPickFiles={() => void pickFiles()}
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

        {dragActive && mode === "queue" ? (
          <div className="drop-overlay" aria-hidden>
            <div className="drop-overlay-inner">
              <FileAudio2 size={40} aria-hidden />
              <p>{t("queue.dropHint")}</p>
            </div>
          </div>
        ) : null}

        <ErrorPanel errors={errors} onDismiss={() => setErrors([])} />

        <main className="content">
          {mode === "dictation" ? (
            <DictationView
              config={config}
              storeReady={storeReady}
              processing={processing}
              dictation={dictation}
              onStart={() => void startDictation()}
              onStop={stopDictation}
              onMicrophoneChange={(name) =>
                void persist((prev) => ({ ...prev, microphoneName: name }))
              }
              onStatus={setStatusMsg}
            />
          ) : (
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
          )}
          <StatusBar
            itemCount={items.length}
            overall={overall}
            modelDownload={modelDownload}
            processing={processing || dictating}
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
            onPickWhisperModelFile={() => void pickModelFile("whisperModel")}
            onPickDictationModelFile={() =>
              void pickModelFile("dictationModel")
            }
            detectedSystemSummaryLang={detectedSystemSummaryLang}
            vulkanAvailable={vulkanAvailable}
            themeMode={themeMode}
            onThemeChange={setThemeMode}
          />
        ) : null}

        {aboutOpen ? (
          <AboutDialog
            version={aboutVersion}
            onClose={() => setAboutOpen(false)}
          />
        ) : null}
      </div>
    </I18nContext.Provider>
  );
}
