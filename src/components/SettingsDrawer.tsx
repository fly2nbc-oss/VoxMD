import { invoke } from "@tauri-apps/api/core";
import { Check, FolderOpen, Languages, Loader2, Mic, Palette, Sparkles, Users } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useT } from "../i18n/I18nProvider";
import {
  UI_LANGUAGE_NAMES,
  UI_LANGUAGES,
  type MessageKey,
  type UiLanguageSetting,
} from "../i18n";
import {
  asMaxSpeakers,
  isSummarySystemLanguage,
  isTranscriptionAuto,
  MAX_SPEAKERS,
} from "../lib/configStore";
import { toMsg } from "../lib/jobs";
import {
  applyProvider,
  isLocalEndpoint,
  LLM_PROVIDER_PRESETS,
  presetFor,
} from "../lib/llmProviders";
import type { ThemeMode } from "../lib/theme";
import type { AppConfig, LlmModelInfo, LlmProvider, WhisperModelInfo } from "../types";
import { LanguagePicker } from "./LanguagePicker";
import { Modal } from "./Modal";

const CUSTOM_MODEL = "__custom__";

/**
 * One panel per tab. Splitting what used to be four stacked sections is what
 * removes the scrollbar: the drawer is now as wide as the window allows and
 * only ever shows one group, so even the tallest panel fits the 560 px minimum
 * window height.
 */
const TABS = [
  { id: "llm", label: "settings.tabLlm", icon: Sparkles },
  { id: "whisper", label: "settings.tabWhisper", icon: Languages },
  { id: "speakers", label: "settings.tabSpeakers", icon: Users },
  { id: "dictation", label: "settings.tabDictation", icon: Mic },
  { id: "appearance", label: "settings.tabAppearance", icon: Palette },
] as const satisfies ReadonlyArray<{ id: string; label: MessageKey; icon: typeof Sparkles }>;

type TabId = (typeof TABS)[number]["id"];

interface Props {
  config: AppConfig;
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
  const { t } = useT();
  const set = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) =>
    onConfigChange({ ...config, [key]: value });

  const [tab, setTab] = useState<TabId>("llm");

  const hasApiKey = config.apiKey.trim() !== "";
  const isPreset = modelInfos.some((m) => m.name === config.whisperModel);
  const showCustomPath = !modelsLoading && !isPreset;
  const dictationIsPreset = modelInfos.some((m) => m.name === config.dictationModel);
  const showDictationCustom = !modelsLoading && !dictationIsPreset;
  const urlLocked = config.llmProvider !== "custom";
  // Judged by the URL, not the preset: a Custom provider may also be local.
  const providerIsLocal = isLocalEndpoint(config.apiBaseUrl);

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

  const modelPicker = (
    <div className="field">
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

  const panels: Record<TabId, React.ReactNode> = {
    llm: (
      <>
        <p className="field-hint settings-intro">{t("settings.llmIntro")}</p>
        <div className="settings-grid">
          <div className="field">
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

          <div className="field">
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

          <div className="field">
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
            ) : null}
          </div>

          {modelPicker}
        </div>

        <div className="field">
          <span className="field-label">{t("settings.summaryLanguage")}</span>
          <p className="field-hint">{t("settings.summaryLanguageHint")}</p>
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
        </div>
      </>
    ),

    whisper: (
      <>
        <p className="field-hint settings-intro">{t("settings.whisperIntro")}</p>
        <div className="settings-grid">
          <div className="field">
            <label className="field-label" htmlFor="wmodel">
              {t("settings.whisperModel")}
            </label>
            <div className="input-with-button">
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
              <button
                type="button"
                className="btn-secondary btn-sm nowrap"
                title={t("settings.clearCacheTitle")}
                disabled={clearingCache || modelInfos.every((m) => !m.cached)}
                onClick={onClearCache}
              >
                {clearingCache ? <Loader2 size={13} className="icon spin" aria-hidden /> : null}
                {clearingCache ? t("settings.clearingCache") : t("settings.clearCache")}
              </button>
            </div>

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

          <div className="field">
            <span className="field-label">{t("settings.transcriptionLanguage")}</span>
            <p className="field-hint">{t("settings.transcriptionLanguageHint")}</p>
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
          </div>

          <div className="field">
            <span className="field-label">{t("settings.gpu")}</span>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={config.useGpu}
                disabled={vulkanAvailable === false}
                onChange={(e) => set("useGpu", e.target.checked)}
              />
              <span>{t("settings.useGpu")}</span>
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
            </label>
            <p className="field-hint">{t("settings.gpuHint")}</p>
          </div>

          <div className="field">
            <span className="field-label">{t("settings.whileProcessing")}</span>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={config.preventSleep}
                onChange={(e) => set("preventSleep", e.target.checked)}
              />
              <span>{t("settings.preventSleep")}</span>
            </label>
            <p className="field-hint">{t("settings.preventSleepHint")}</p>
          </div>
        </div>
      </>
    ),

    speakers: (
      <>
        <div className="field">
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={config.diarizationEnabled}
              onChange={(e) => set("diarizationEnabled", e.target.checked)}
            />
            <span>{t("settings.speakerLabels")}</span>
          </label>
        </div>
        <div className="field settings-narrow">
          <label className="field-label" htmlFor="maxSpeakers">
            {t("settings.speakerCount")}
          </label>
          <input
            id="maxSpeakers"
            className="input"
            type="number"
            min={0}
            max={MAX_SPEAKERS}
            value={config.maxSpeakers}
            disabled={!config.diarizationEnabled}
            onChange={(e) => set("maxSpeakers", asMaxSpeakers(Number(e.target.value)))}
          />
        </div>
        <p className="field-hint">{t("settings.speakersHint")}</p>
        <p className="field-hint">{t("settings.speakersHint2", { max: MAX_SPEAKERS })}</p>
      </>
    ),

    dictation: (
      <>
        <p className="field-hint settings-intro">{t("settings.dictationIntro")}</p>
        <div className="field settings-narrow">
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
        </div>
      </>
    ),

    appearance: (
      <>
        <div className="settings-grid">
          <div className="field">
            <span className="field-label">{t("settings.theme")}</span>
            <div className="lang-option-row">
              {(["system", "light", "dark"] as const).map((mode) => (
                <label className="lang-radio" key={mode}>
                  <input
                    type="radio"
                    name="themeMode"
                    checked={themeMode === mode}
                    onChange={() => onThemeChange(mode)}
                  />
                  <span>
                    {mode === "system"
                      ? t("settings.themeSystem")
                      : mode === "light"
                        ? t("settings.themeLight")
                        : t("settings.themeDark")}
                  </span>
                </label>
              ))}
            </div>
          </div>

          <div className="field">
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
        <p className="field-hint shortcuts-hint">{t("settings.shortcuts")}</p>
      </>
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
        <nav className="settings-tabs" role="tablist" aria-label={t("settings.sections")}>
          {TABS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              id={`settings-tab-${id}`}
              aria-selected={tab === id}
              aria-controls={`settings-panel-${id}`}
              className={`settings-tab${tab === id ? " is-active" : ""}`}
              onClick={() => setTab(id)}
            >
              <Icon size={16} aria-hidden />
              <span>{t(label)}</span>
            </button>
          ))}
        </nav>

        <section
          className="settings-panel"
          role="tabpanel"
          id={`settings-panel-${tab}`}
          aria-labelledby={`settings-tab-${tab}`}
        >
          {panels[tab]}
        </section>
      </div>

      {saveError ? <p className="form-error">{saveError}</p> : null}

      <div className="settings-actions">
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
        <button
          type="button"
          className="btn-secondary"
          title={t("settings.resetTitle")}
          onClick={onResetDefaults}
        >
          {t("settings.reset")}
        </button>
      </div>
    </Modal>
  );
}
