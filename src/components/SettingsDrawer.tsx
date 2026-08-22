import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  FolderOpen,
  Info,
  Languages,
  Loader2,
  Mic,
  Palette,
  Search,
  Sparkles,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import appIcon from "../../src-tauri/icons/128x128.png";
import { useT } from "../i18n/I18nProvider";
import {
  UI_LANGUAGE_NAMES,
  UI_LANGUAGES,
  type MessageKey,
  type UiLanguageSetting,
} from "../i18n";
import { asMaxSpeakers, isSummarySystemLanguage, isTranscriptionAuto } from "../lib/configStore";
import { toMsg } from "../lib/jobs";
import {
  applyProvider,
  isLocalEndpoint,
  LLM_PROVIDER_PRESETS,
  presetFor,
} from "../lib/llmProviders";
import {
  changedFields,
  changedSections,
  searchSettings,
  SETTINGS_SECTIONS,
  type SettingsTab,
} from "../lib/settingsSearch";
import type { ThemeMode } from "../lib/theme";
import type { AppConfig, LlmModelInfo, LlmProvider, WhisperModelInfo } from "../types";
import { LanguagePicker } from "./LanguagePicker";
import { Modal } from "./Modal";

const CUSTOM_MODEL = "__custom__";
const GITHUB_URL = "https://github.com/fly2nbc-oss/VoxMD";

const TAB_ICON: Record<SettingsTab, typeof Sparkles> = {
  appearance: Palette,
  whisper: Languages,
  llm: Sparkles,
  dictation: Mic,
  about: Info,
};

const TAB_LABEL: Record<SettingsTab, MessageKey> = {
  appearance: "settings.tabAppearance",
  whisper: "settings.tabWhisper",
  llm: "settings.tabLlm",
  dictation: "settings.tabDictation",
  about: "settings.tabAbout",
};

interface Props {
  config: AppConfig;
  /** What is on disk — the drawer diffs against it for the unsaved indicator. */
  savedConfig: AppConfig;
  onConfigChange: (next: AppConfig) => void;
  storeReady: boolean;
  saveState: "idle" | "saving" | "saved";
  saveError: string;
  onSave: () => void;
  onResetDefaults: () => void;
  onClose: () => void;

  modelInfos: WhisperModelInfo[];
  modelsLoading: boolean;
  clearingCache: boolean;
  onClearCache: () => void;
  onPickWhisperModelFile: () => void;
  onPickDictationModelFile: () => void;

  detectedSystemSummaryLang: string;
  vulkanAvailable: boolean | null;
  themeMode: ThemeMode;
  onThemeChange: (mode: ThemeMode) => void;
}

export function SettingsDrawer({
  config,
  savedConfig,
  onConfigChange,
  storeReady,
  saveState,
  saveError,
  onSave,
  onResetDefaults,
  onClose,
  modelInfos,
  modelsLoading,
  clearingCache,
  onClearCache,
  onPickWhisperModelFile,
  onPickDictationModelFile,
  detectedSystemSummaryLang,
  vulkanAvailable,
  themeMode,
  onThemeChange,
}: Props) {
  const { t, tn } = useT();
  const set = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) =>
    onConfigChange({ ...config, [key]: value });

  const [tab, setTab] = useState<SettingsTab>("appearance");
  const [query, setQuery] = useState("");
  /** Field ringed after a search jump, so the eye lands on the right control. */
  const [highlight, setHighlight] = useState("");
  const highlightTimer = useRef<number | undefined>(undefined);
  const [version, setVersion] = useState("");

  useEffect(() => () => window.clearTimeout(highlightTimer.current), []);

  useEffect(() => {
    if (tab !== "about" || version) return;
    void getVersion()
      .then(setVersion)
      .catch(() => setVersion("—"));
  }, [tab, version]);

  const hasApiKey = config.apiKey.trim() !== "";
  const isPreset = modelInfos.some((m) => m.name === config.whisperModel);
  const showCustomPath = !modelsLoading && !isPreset;
  const dictationIsPreset = modelInfos.some((m) => m.name === config.dictationModel);
  const showDictationCustom = !modelsLoading && !dictationIsPreset;
  const urlLocked = config.llmProvider !== "custom";
  // Judged by the URL, not the preset: a Custom provider may also be local.
  const providerIsLocal = isLocalEndpoint(config.apiBaseUrl);
  const cachedCount = modelInfos.filter((m) => m.cached).length;

  const dirtyFields = useMemo(
    () => changedFields(config, savedConfig),
    [config, savedConfig],
  );
  const dirtySections = useMemo(
    () => changedSections(config, savedConfig),
    [config, savedConfig],
  );
  const results = useMemo(() => searchSettings(query, t), [query, t]);
  const searching = query.trim().length > 0;

  const jumpTo = (section: SettingsTab, id: string) => {
    setTab(section);
    setQuery("");
    setHighlight(id);
    window.clearTimeout(highlightTimer.current);
    highlightTimer.current = window.setTimeout(() => setHighlight(""), 2200);
  };

  /** Ring drawn with `outline`, which paints outside the box and moves nothing. */
  const ring = (id: string) => (highlight === id ? " is-found" : "");

  const [llmModels, setLlmModels] = useState<LlmModelInfo[]>([]);
  const [llmModelsLoading, setLlmModelsLoading] = useState(
    () =>
      config.apiBaseUrl.trim() !== "" &&
      (config.apiKey.trim() !== "" || isLocalEndpoint(config.apiBaseUrl)),
  );
  const [verifying, setVerifying] = useState(false);
  const [verifyMsg, setVerifyMsg] = useState("");
  const [verifyOk, setVerifyOk] = useState<boolean | null>(null);
  const [providerSwitched, setProviderSwitched] = useState(false);

  const loadLlmModels = useCallback(async (cfg: AppConfig) => {
    if ((!cfg.apiKey.trim() && !isLocalEndpoint(cfg.apiBaseUrl)) || !cfg.apiBaseUrl.trim()) {
      setLlmModels([]);
      setLlmModelsLoading(false);
      return;
    }
    setLlmModelsLoading(true);
    try {
      setLlmModels(await invoke<LlmModelInfo[]>("list_llm_models", { config: cfg }));
    } catch {
      setLlmModels([]);
    } finally {
      setLlmModelsLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      if (
        (!config.apiKey.trim() && !isLocalEndpoint(config.apiBaseUrl)) ||
        !config.apiBaseUrl.trim()
      ) {
        if (!cancelled) setLlmModelsLoading(false);
        return;
      }
      try {
        const models = await invoke<LlmModelInfo[]>("list_llm_models", { config });
        if (!cancelled) setLlmModels(models);
      } catch {
        if (!cancelled) setLlmModels([]);
      } finally {
        if (!cancelled) setLlmModelsLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // Snapshot at drawer open; Verify / provider change refresh explicitly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onProviderChange = (id: LlmProvider) => {
    const next = applyProvider(config, id);
    onConfigChange(next);
    setVerifyMsg("");
    setVerifyOk(null);
    // Keys are per provider. Keeping the old one is what makes the next Verify
    // fail with an opaque 401, so say so instead.
    setProviderSwitched(id !== config.llmProvider && next.apiKey.trim() !== "");
    void loadLlmModels(next);
  };

  const verifyKey = async () => {
    setVerifying(true);
    setVerifyMsg("");
    setVerifyOk(null);
    try {
      await invoke("verify_api_key", { config });
      setVerifyOk(true);
      setVerifyMsg(t("settings.keyAccepted"));
      await loadLlmModels(config);
    } catch (e) {
      setVerifyOk(false);
      setVerifyMsg(toMsg(e));
    } finally {
      setVerifying(false);
    }
  };

  const modelInList = llmModels.some((m) => m.id === config.apiModel);
  const showModelSelect = llmModels.length > 0;

  /** Current value shown under each rail label, so the rail reads as a summary. */
  const railSummary = (section: SettingsTab): string => {
    const whisperLabel =
      modelInfos.find((m) => m.name === config.whisperModel)?.name ?? config.whisperModel;
    switch (section) {
      case "appearance":
        return `${t(
          themeMode === "system"
            ? "settings.themeSystem"
            : themeMode === "light"
              ? "settings.themeLight"
              : "settings.themeDark",
        )} · ${
          config.uiLanguage === "system"
            ? t("settings.uiLanguageSystem")
            : UI_LANGUAGE_NAMES[config.uiLanguage]
        }`;
      case "whisper":
        return `${whisperLabel} · ${t("settings.speakerSection")} ${
          config.diarizationEnabled
            ? config.maxSpeakers === 0
              ? t("settings.speakerAuto").toLowerCase()
              : String(config.maxSpeakers)
            : t("common.off")
        }`;
      case "llm":
        return `${presetFor(config.llmProvider).label} · ${config.apiModel}`;
      case "dictation":
        return (
          modelInfos.find((m) => m.name === config.dictationModel)?.name ?? config.dictationModel
        );
      default:
        return "";
    }
  };

  const railButton = (section: SettingsTab) => {
    const Icon = TAB_ICON[section];
    const active = tab === section && !searching;
    const summary = railSummary(section);
    return (
      <button
        key={section}
        type="button"
        role="tab"
        id={`settings-tab-${section}`}
        aria-selected={active}
        aria-controls={`settings-panel-${section}`}
        className={`settings-tab${active ? " is-active" : ""}`}
        onClick={() => {
          setTab(section);
          setHighlight("");
        }}
      >
        <Icon size={16} aria-hidden />
        <span className="settings-tab-text">
          <span className="settings-tab-label">{t(TAB_LABEL[section])}</span>
          {summary ? <span className="settings-tab-value">{summary}</span> : null}
        </span>
        {dirtySections.has(section as never) ? (
          <span className="settings-tab-dot" title={t("settings.unsavedDot")} />
        ) : (
          <span className="settings-tab-dot-slot" aria-hidden />
        )}
      </button>
    );
  };

  const modelPicker = (
    <div className={`field${ring("apiModel")}`}>
      <label className="field-label" htmlFor="model">
        {t("settings.model")}
      </label>
      {showModelSelect ? (
        <select
          id="model"
          className="input"
          value={modelInList ? config.apiModel : ""}
          disabled={!config.includeSummary || llmModelsLoading}
          onChange={(e) => {
            if (e.target.value) set("apiModel", e.target.value);
          }}
        >
          {!modelInList ? (
            <option value="">
              {config.apiModel
                ? t("settings.currentModel", { model: config.apiModel })
                : t("settings.selectModel")}
            </option>
          ) : null}
          {llmModels.map((m) => (
            <option key={m.id} value={m.id}>
              {m.id}
            </option>
          ))}
        </select>
      ) : (
        <input
          id="model"
          className="input"
          placeholder={presetFor(config.llmProvider).defaultModel || "model-id"}
          value={config.apiModel}
          disabled={!config.includeSummary}
          onChange={(e) => set("apiModel", e.target.value)}
        />
      )}
      {showModelSelect ? (
        <input
          className="input custom-model-row"
          placeholder={t("settings.modelId")}
          value={config.apiModel}
          disabled={!config.includeSummary}
          onChange={(e) => set("apiModel", e.target.value)}
          aria-label={t("settings.modelId")}
        />
      ) : null}
      <p className="field-hint">
        {llmModelsLoading
          ? t("settings.modelsLoading")
          : showModelSelect
            ? t("settings.modelHintCatalog")
            : t("settings.modelHintFree")}
      </p>
    </div>
  );

  const panels: Record<SettingsTab, React.ReactNode> = {
    appearance: (
      <>
        <div className="settings-grid">
          <div className={`field${ring("theme")}`}>
            <span className="field-label">{t("settings.theme")}</span>
            <div className="segmented">
              {(["system", "light", "dark"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  className={`segmented-btn${themeMode === mode ? " is-active" : ""}`}
                  aria-pressed={themeMode === mode}
                  onClick={() => onThemeChange(mode)}
                >
                  {mode === "system"
                    ? t("settings.themeSystem")
                    : mode === "light"
                      ? t("settings.themeLight")
                      : t("settings.themeDark")}
                </button>
              ))}
            </div>
          </div>

          <div className={`field${ring("uiLanguage")}`}>
            <label className="field-label" htmlFor="uiLanguage">
              {t("settings.uiLanguage")}
            </label>
            <select
              id="uiLanguage"
              className="input"
              value={config.uiLanguage}
              onChange={(e) => set("uiLanguage", e.target.value as UiLanguageSetting)}
            >
              <option value="system">{t("settings.uiLanguageSystem")}</option>
              {UI_LANGUAGES.map((code) => (
                <option key={code} value={code}>
                  {UI_LANGUAGE_NAMES[code]}
                </option>
              ))}
            </select>
            <p className="field-hint">{t("settings.uiLanguageHint")}</p>
          </div>
        </div>

        <div className="field">
          <span className="field-label">{t("settings.shortcutsTitle")}</span>
          <div className="shortcut-grid">
            {(
              [
                ["shortcut.start", "F5"],
                ["shortcut.cancel", "Esc"],
                ["shortcut.files", "Ctrl + O"],
                ["shortcut.settings", "Ctrl + ,"],
                ["shortcut.queue", "Ctrl + 1"],
                ["shortcut.dictation", "Ctrl + 2"],
              ] as Array<[MessageKey, string]>
            ).map(([key, combo]) => (
              <div className="shortcut-row" key={combo}>
                <span>{t(key)}</span>
                <kbd>{combo}</kbd>
              </div>
            ))}
          </div>
        </div>
      </>
    ),

    whisper: (
      <>
        <p className="field-hint settings-intro">{t("settings.whisperIntro")}</p>
        <div className="settings-grid">
          <div className={`field${ring("whisperModel")}`}>
            <label className="field-label" htmlFor="wmodel">
              {t("settings.whisperModel")}
            </label>
            <select
              id="wmodel"
              className="input"
              value={isPreset ? config.whisperModel : CUSTOM_MODEL}
              disabled={modelsLoading}
              onChange={(e) => {
                if (e.target.value === CUSTOM_MODEL) {
                  if (isPreset) set("whisperModel", "");
                  return;
                }
                set("whisperModel", e.target.value);
              }}
            >
              {modelInfos.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.name} · {m.sizeHint}
                  {m.cached ? " ✓" : ""}
                </option>
              ))}
              <option value={CUSTOM_MODEL}>{t("settings.customPath")}</option>
            </select>
            {showCustomPath ? (
              <div className="input-with-button custom-model-row">
                <input
                  className="input"
                  placeholder={t("settings.modelPathPlaceholder")}
                  value={config.whisperModel}
                  onChange={(e) => set("whisperModel", e.target.value)}
                  aria-label={t("settings.whisperPathAria")}
                />
                <button
                  type="button"
                  className="btn-secondary btn-sm"
                  title={t("settings.choosePathTitle")}
                  onClick={onPickWhisperModelFile}
                >
                  <FolderOpen size={16} aria-hidden />
                  <span>{t("common.choose")}</span>
                </button>
              </div>
            ) : null}
            <p className="field-hint">
              {isPreset || modelsLoading
                ? t("settings.whisperHintPreset")
                : t("settings.whisperHintCustom")}
            </p>
          </div>

          <div className={`field${ring("language")}`}>
            <span className="field-label">{t("settings.transcriptionLanguage")}</span>
            <LanguagePicker
              name="transcriptionLangMode"
              value={config.language}
              defaultValue="auto"
              defaultLabel={t("lang.autoDetect")}
              isDefault={isTranscriptionAuto}
              isoAriaLabel={t("lang.transcriptionIsoAria")}
              isoFallback={() => "de"}
              onChange={(v) => set("language", v)}
            />
            <p className="field-hint">{t("settings.transcriptionLanguageHint")}</p>
          </div>
        </div>

        <div className="settings-block">
          <span className="field-label">{t("settings.speakerSection")}</span>
          <div className="settings-grid">
            <label className={`option-card${ring("diarizationEnabled")}`}>
              <input
                type="checkbox"
                checked={config.diarizationEnabled}
                onChange={(e) => set("diarizationEnabled", e.target.checked)}
              />
              <span>
                <span className="option-title">{t("settings.speakerLabels")}</span>
                <span className="field-hint">{t("settings.speakersHint")}</span>
              </span>
            </label>

            <div
              className={`field${ring("maxSpeakers")}`}
              style={{ opacity: config.diarizationEnabled ? 1 : 0.45 }}
            >
              <span className="option-title">{t("settings.speakerCountLabel")}</span>
              <div className="segmented segmented-wrap">
                {[0, 2, 3, 4, 5].map((n) => (
                  <button
                    key={n}
                    type="button"
                    className={`segmented-btn${config.maxSpeakers === n ? " is-active" : ""}`}
                    aria-pressed={config.maxSpeakers === n}
                    disabled={!config.diarizationEnabled}
                    onClick={() => set("maxSpeakers", asMaxSpeakers(n))}
                  >
                    {n === 0 ? t("settings.speakerAuto") : n === 5 ? t("settings.speakerMore") : n}
                  </button>
                ))}
              </div>
              <p className="field-hint">
                {config.maxSpeakers === 0
                  ? t("settings.speakerAutoNote")
                  : t("settings.speakerFixedNote")}
              </p>
            </div>
          </div>
        </div>

        <div className="settings-block">
          <span className="field-label">{t("settings.onThisMachine")}</span>
          <div className="settings-grid">
            <label className={`option-card${ring("useGpu")}`}>
              <input
                type="checkbox"
                checked={config.useGpu}
                disabled={vulkanAvailable === false}
                onChange={(e) => set("useGpu", e.target.checked)}
              />
              <span>
                <span className="option-title">
                  {t("settings.useGpu")}
                  <span
                    className={`badge ${
                      vulkanAvailable === true
                        ? "badge-ok"
                        : vulkanAvailable === false
                          ? "badge-warn"
                          : "badge-neutral"
                    }`}
                    title={t("settings.gpuBadgeTitle")}
                  >
                    {vulkanAvailable === true
                      ? t("settings.gpuAvailable")
                      : vulkanAvailable === false
                        ? t("settings.gpuCpuOnly")
                        : t("settings.gpuChecking")}
                  </span>
                </span>
                <span className="field-hint">{t("settings.gpuHint")}</span>
              </span>
            </label>

            <label className={`option-card${ring("preventSleep")}`}>
              <input
                type="checkbox"
                checked={config.preventSleep}
                onChange={(e) => set("preventSleep", e.target.checked)}
              />
              <span>
                <span className="option-title">{t("settings.preventSleep")}</span>
                <span className="field-hint">{t("settings.preventSleepHint")}</span>
              </span>
            </label>
          </div>

          <div className={`cache-row${ring("whisperCache")}`}>
            <span>
              <span className="option-title">{t("settings.modelCacheLine")}</span>
              <span className="field-hint">
                {cachedCount === 0
                  ? t("settings.modelCacheNone")
                  : t("settings.modelCacheSome", { count: cachedCount })}
              </span>
            </span>
            <button
              type="button"
              className="btn-secondary btn-sm nowrap"
              title={t("settings.clearCacheTitle")}
              disabled={clearingCache || cachedCount === 0}
              onClick={onClearCache}
            >
              {clearingCache ? <Loader2 size={13} className="icon spin" aria-hidden /> : null}
              {clearingCache ? t("settings.clearingCache") : t("settings.freeSpace")}
            </button>
          </div>
        </div>
      </>
    ),

    llm: (
      <>
        <p className="field-hint settings-intro">{t("settings.llmIntro")}</p>
        <div className="settings-grid">
          <div className={`field${ring("llmProvider")}`}>
            <label className="field-label" htmlFor="llmProvider">
              {t("settings.provider")}
            </label>
            <select
              id="llmProvider"
              className="input"
              value={config.llmProvider}
              disabled={!config.includeSummary}
              onChange={(e) => onProviderChange(e.target.value as LlmProvider)}
            >
              {LLM_PROVIDER_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
            <p className="field-hint">{t("settings.providerHint")}</p>
          </div>

          <div className={`field${ring("apiBaseUrl")}`}>
            <label className="field-label" htmlFor="apiBase">
              {t("settings.baseUrl")}
            </label>
            <input
              id="apiBase"
              className="input"
              placeholder="https://api.deepseek.com"
              value={config.apiBaseUrl}
              disabled={!config.includeSummary || urlLocked}
              onChange={(e) => set("apiBaseUrl", e.target.value)}
            />
            <p className="field-hint">{t("settings.baseUrlHint")}</p>
          </div>

          <div className={`field${ring("apiKey")}`}>
            <label className="field-label" htmlFor="apiKey">
              {t("settings.apiKey")}
            </label>
            <div className="input-with-button">
              <input
                id="apiKey"
                className="input"
                type="password"
                autoComplete="off"
                value={config.apiKey}
                disabled={!config.includeSummary}
                onChange={(e) => {
                  setProviderSwitched(false);
                  set("apiKey", e.target.value);
                }}
              />
              <button
                type="button"
                className="btn-secondary btn-sm nowrap"
                disabled={!config.includeSummary || (!hasApiKey && !providerIsLocal) || verifying}
                onClick={() => void verifyKey()}
                title={t("settings.verifyTitle")}
              >
                {verifying ? <Loader2 size={13} className="icon spin" aria-hidden /> : null}
                {verifying ? t("settings.verifying") : t("settings.verify")}
              </button>
            </div>
            {verifyMsg ? (
              <p className={`field-hint${verifyOk === false ? " field-hint-warn" : ""}`}>
                {verifyMsg}
              </p>
            ) : providerSwitched ? (
              <p className="field-hint field-hint-warn">{t("settings.providerSwitched")}</p>
            ) : config.includeSummary && !hasApiKey && !providerIsLocal ? (
              <p className="field-hint field-hint-warn">{t("settings.needKey")}</p>
            ) : !config.includeSummary ? (
              <p className="field-hint">{t("settings.summaryOff")}</p>
            ) : (
              <p className="field-hint">{t("search.apiKeyHint")}</p>
            )}
          </div>

          {modelPicker}
        </div>

        <div className={`field${ring("summaryLanguage")}`}>
          <span className="field-label">{t("settings.summaryLanguage")}</span>
          <LanguagePicker
            name="summaryLangMode"
            value={config.summaryLanguage}
            defaultValue="system"
            defaultLabel={t("lang.systemLanguage")}
            isDefault={isSummarySystemLanguage}
            detected={detectedSystemSummaryLang}
            isoAriaLabel={t("lang.summaryIsoAria")}
            disabled={!config.includeSummary}
            isoFallback={() =>
              (isTranscriptionAuto(config.language) ? "" : config.language) ||
              detectedSystemSummaryLang
            }
            onChange={(v) => set("summaryLanguage", v)}
          />
          <p className="field-hint">{t("settings.summaryLanguageHint")}</p>
        </div>
      </>
    ),

    dictation: (
      <>
        <p className="field-hint settings-intro">{t("settings.dictationIntro")}</p>
        <div className={`field settings-narrow${ring("dictationModel")}`}>
          <label className="field-label" htmlFor="dmodel">
            {t("settings.dictationModel")}
          </label>
          <select
            id="dmodel"
            className="input"
            value={dictationIsPreset ? config.dictationModel : CUSTOM_MODEL}
            disabled={modelsLoading}
            onChange={(e) => {
              if (e.target.value === CUSTOM_MODEL) {
                if (dictationIsPreset) set("dictationModel", "");
                return;
              }
              set("dictationModel", e.target.value);
            }}
          >
            {modelInfos.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name} · {m.sizeHint}
                {m.cached ? " ✓" : ""}
              </option>
            ))}
            <option value={CUSTOM_MODEL}>{t("settings.customPath")}</option>
          </select>
          {showDictationCustom ? (
            <div className="input-with-button custom-model-row">
              <input
                className="input"
                placeholder={t("settings.modelPathPlaceholder")}
                value={config.dictationModel}
                onChange={(e) => set("dictationModel", e.target.value)}
                aria-label={t("settings.dictationPathAria")}
              />
              <button
                type="button"
                className="btn-secondary btn-sm"
                title={t("settings.choosePathTitle")}
                onClick={onPickDictationModelFile}
              >
                <FolderOpen size={16} aria-hidden />
                <span>{t("common.choose")}</span>
              </button>
            </div>
          ) : null}
          <p className="field-hint">{t("search.dictationModelHint")}</p>
        </div>
      </>
    ),

    about: (
      <div className="about-panel">
        <img src={appIcon} alt="" width={96} height={96} className="about-app-icon" />
        <p className="about-tagline">{t("about.tagline")}</p>
        <p className="about-version">
          <span className="muted-text">{t("about.version")}</span>{" "}
          <span className="mono">{version || "…"}</span>
        </p>
        <div>
          <div className="about-section-label">{t("about.repo")}</div>
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
    ),
  };

  return (
    <Modal
      title={t("settings.title")}
      onClose={onClose}
      variant="drawer"
      panelClassName="drawer-wide"
      bodyClassName="settings-body"
    >
      <div className="settings-layout">
        <div className="settings-rail">
          <div className="settings-search">
            <Search size={15} aria-hidden />
            <input
              className="input"
              value={query}
              placeholder={t("settings.search")}
              aria-label={t("settings.search")}
              onChange={(e) => setQuery(e.target.value)}
            />
            {searching ? (
              <button
                type="button"
                className="icon-btn"
                title={t("settings.searchClear")}
                aria-label={t("settings.searchClear")}
                onClick={() => setQuery("")}
              >
                <X size={14} aria-hidden />
              </button>
            ) : null}
          </div>

          <nav className="settings-tabs" role="tablist" aria-label={t("settings.sections")}>
            {SETTINGS_SECTIONS.map(railButton)}
            <span className="settings-tabs-gap" aria-hidden />
            {railButton("about")}
          </nav>
        </div>

        {searching ? (
          <section className="settings-panel" aria-label={t("settings.search")}>
            <p className="field-hint">
              {tn("settings.resultsOne", "settings.resultsMany", results.length)}
            </p>
            {results.length === 0 ? (
              <p className="field-hint">{t("settings.noResults")}</p>
            ) : (
              <div className="search-hits">
                {results.map((hit) => (
                  <button
                    key={hit.id}
                    type="button"
                    className="search-hit"
                    onClick={() => jumpTo(hit.section, hit.id)}
                  >
                    <span className="search-hit-head">
                      <span className="search-hit-label">{t(hit.label)}</span>
                      <span className="search-hit-section">{t(TAB_LABEL[hit.section])}</span>
                    </span>
                    <span className="field-hint">{t(hit.hint)}</span>
                  </button>
                ))}
              </div>
            )}
          </section>
        ) : (
          <section
            className="settings-panel"
            role="tabpanel"
            id={`settings-panel-${tab}`}
            aria-labelledby={`settings-tab-${tab}`}
          >
            {panels[tab]}
          </section>
        )}
      </div>

      {saveError ? <p className="form-error">{saveError}</p> : null}

      <div className="settings-actions">
        <span className={`settings-dirty${dirtyFields.length ? " is-dirty" : ""}`}>
          {dirtyFields.length ? <span className="settings-tab-dot" aria-hidden /> : null}
          {saveState === "saved"
            ? t("settings.justSaved")
            : dirtyFields.length === 0
              ? t("settings.unsavedNone")
              : tn("settings.unsavedOne", "settings.unsavedMany", dirtyFields.length)}
        </span>
        <button
          type="button"
          className="btn-secondary"
          title={t("settings.resetTitle")}
          onClick={onResetDefaults}
        >
          {t("settings.reset")}
        </button>
        <button
          type="button"
          className="btn-primary"
          disabled={!storeReady || saveState !== "idle"}
          onClick={onSave}
        >
          {saveState === "saved" ? (
            <>
              <Check size={15} aria-hidden /> {t("common.saved")}
            </>
          ) : saveState === "saving" ? (
            <>
              <Loader2 size={14} className="icon spin" aria-hidden /> {t("common.saving")}
            </>
          ) : (
            t("common.save")
          )}
        </button>
      </div>
    </Modal>
  );
}
