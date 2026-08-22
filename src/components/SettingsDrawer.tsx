import { Check, FolderOpen, Loader2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import {
  asMaxSpeakers,
  isSummarySystemLanguage,
  isTranscriptionAuto,
  MAX_SPEAKERS,
} from "../lib/configStore";
import { toMsg } from "../lib/jobs";
import { applyProvider, LLM_PROVIDER_PRESETS } from "../lib/llmProviders";
import type { ThemeMode } from "../lib/theme";
import type { AppConfig, LlmModelInfo, LlmProvider, WhisperModelInfo } from "../types";
import { LanguagePicker } from "./LanguagePicker";
import { Modal } from "./Modal";

const CUSTOM_MODEL = "__custom__";

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
  const set = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) =>
    onConfigChange({ ...config, [key]: value });

  const hasApiKey = config.apiKey.trim() !== "";
  const isPreset = modelInfos.some((m) => m.name === config.whisperModel);
  const showCustomPath = !modelsLoading && !isPreset;
  const dictationIsPreset = modelInfos.some((m) => m.name === config.dictationModel);
  const showDictationCustom = !modelsLoading && !dictationIsPreset;
  const urlLocked = config.llmProvider !== "custom";

  const [llmModels, setLlmModels] = useState<LlmModelInfo[]>([]);
  const [llmModelsLoading, setLlmModelsLoading] = useState(
    () => config.apiKey.trim() !== "" && config.apiBaseUrl.trim() !== "",
  );
  const [verifying, setVerifying] = useState(false);
  const [verifyMsg, setVerifyMsg] = useState("");
  const [verifyOk, setVerifyOk] = useState<boolean | null>(null);

  const loadLlmModels = useCallback(async (cfg: AppConfig) => {
    if (!cfg.apiKey.trim() || !cfg.apiBaseUrl.trim()) {
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
      if (!config.apiKey.trim() || !config.apiBaseUrl.trim()) {
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

  const [providerSwitched, setProviderSwitched] = useState(false);

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
      setVerifyMsg("Key accepted.");
      await loadLlmModels(config);
    } catch (e) {
      setVerifyOk(false);
      setVerifyMsg(toMsg(e));
    } finally {
      setVerifying(false);
    }
  };

  const modelInList = llmModels.some((m) => m.id === config.apiModel);
  const showModelSelect = config.llmProvider === "openrouter" || llmModels.length > 0;

  return (
    <Modal title="Settings" onClose={onClose} variant="drawer">
      <section className="settings-section">
        <h2 className="settings-section-title">Summary (LLM)</h2>
        <p className="field-hint">
          OpenAI-compatible chat API for the Markdown summary. Only used when Summary is enabled in
          the toolbar. Without a key, transcription still works; the summary section is skipped.
        </p>

        <div className="field">
          <label className="field-label" htmlFor="llmProvider">
            Provider
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
          <p className="field-hint">
            Deepseek and OpenRouter fill the base URL. Custom leaves URL and model as free text.
          </p>
        </div>

        <div className="field">
          <label className="field-label" htmlFor="apiKey">
            API key
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
              disabled={!config.includeSummary || !hasApiKey || verifying}
              onClick={() => void verifyKey()}
              title="Check the key against the provider’s model list"
            >
              {verifying ? <Loader2 size={13} className="icon spin" aria-hidden /> : null}
              {verifying ? "Checking…" : "Verify"}
            </button>
          </div>
          {verifyMsg ? (
            <p className={`field-hint${verifyOk === false ? " field-hint-warn" : ""}`}>{verifyMsg}</p>
          ) : providerSwitched ? (
            <p className="field-hint field-hint-warn">
              This key was entered for the previous provider — replace it and press Verify.
            </p>
          ) : config.includeSummary && !hasApiKey ? (
            <p className="field-hint field-hint-warn">Enter a key to generate summaries.</p>
          ) : !config.includeSummary ? (
            <p className="field-hint">Summary is off in the toolbar — these fields are unused.</p>
          ) : null}
        </div>

        <div className="field">
          <label className="field-label" htmlFor="apiBase">
            API base URL
          </label>
          <input
            id="apiBase"
            className="input"
            placeholder="https://api.deepseek.com"
            value={config.apiBaseUrl}
            disabled={!config.includeSummary || urlLocked}
            onChange={(e) => set("apiBaseUrl", e.target.value)}
          />
          <p className="field-hint">
            Endpoint root, without <code>/chat/completions</code>. OpenRouter needs{" "}
            <code>/api/v1</code>.
          </p>
        </div>

        <div className="field">
          <label className="field-label" htmlFor="model">
            Model
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
                  {config.apiModel ? `Current: ${config.apiModel}` : "Select a model…"}
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
              placeholder="deepseek-chat"
              value={config.apiModel}
              disabled={!config.includeSummary}
              onChange={(e) => set("apiModel", e.target.value)}
            />
          )}
          {config.llmProvider === "custom" && showModelSelect ? (
            <input
              className="input custom-model-row"
              placeholder="Model id"
              value={config.apiModel}
              disabled={!config.includeSummary}
              onChange={(e) => set("apiModel", e.target.value)}
              aria-label="Model id"
            />
          ) : null}
          <p className="field-hint">
            {llmModelsLoading
              ? "Loading models…"
              : config.llmProvider === "openrouter"
                ? "Chat models from OpenRouter, alphabetically. Verify the key to refresh the list."
                : "Model id as expected by that provider."}
          </p>
        </div>

        <div className="field">
          <span className="field-label">Summary language</span>
          <p className="field-hint">Language of the written summary (not the spoken audio).</p>
          <LanguagePicker
            name="summaryLangMode"
            value={config.summaryLanguage}
            defaultValue="system"
            defaultLabel="System language"
            isDefault={isSummarySystemLanguage}
            detected={detectedSystemSummaryLang}
            isoAriaLabel="Summary language ISO code"
            disabled={!config.includeSummary}
            isoFallback={() =>
              (isTranscriptionAuto(config.language) ? "" : config.language) ||
              detectedSystemSummaryLang
            }
            onChange={(v) => set("summaryLanguage", v)}
          />
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-section-title">Transcription (Whisper)</h2>
        <p className="field-hint">
          Local speech-to-text on this device. Produces the transcript section of the Markdown
          output.
        </p>

        <div className="field">
          <label className="field-label" htmlFor="wmodel">
            Whisper model
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
              <option value={CUSTOM_MODEL}>Custom path…</option>
            </select>
            <button
              type="button"
              className="btn-secondary btn-sm nowrap"
              title="Delete all downloaded Whisper models from the local cache"
              disabled={clearingCache || modelInfos.every((m) => !m.cached)}
              onClick={onClearCache}
            >
              {clearingCache ? <Loader2 size={13} className="icon spin" aria-hidden /> : null}
              {clearingCache ? "Deleting…" : "Clear cache"}
            </button>
          </div>

          {showCustomPath ? (
            <div className="input-with-button custom-model-row">
              <input
                className="input"
                placeholder="/absolute/path/to/model.bin"
                value={config.whisperModel}
                onChange={(e) => set("whisperModel", e.target.value)}
                aria-label="Path to a local Whisper model file"
              />
              <button
                type="button"
                className="btn-secondary btn-sm"
                title="Choose a local .bin or .gguf model file"
                onClick={onPickWhisperModelFile}
              >
                <FolderOpen size={16} aria-hidden />
                <span>Choose…</span>
              </button>
            </div>
          ) : null}

          <p className="field-hint">
            {isPreset || modelsLoading
              ? "Larger models are slower but usually more accurate. Named models download on first use (✓ = already cached)."
              : "Point to your own Whisper weights (.bin / .gguf)."}
          </p>
        </div>

        <div className="field">
          <span className="field-label">Transcription language</span>
          <p className="field-hint">
            Spoken language in the audio. Auto-detect works well; ISO is faster when you know it.
          </p>
          <LanguagePicker
            name="transcriptionLangMode"
            value={config.language}
            defaultValue="auto"
            defaultLabel="Auto-detect"
            isDefault={isTranscriptionAuto}
            isoAriaLabel="Transcription language ISO code"
            isoFallback={() => "de"}
            onChange={(v) => set("language", v)}
          />
        </div>

        <div className="field">
          <span className="field-label">GPU</span>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={config.useGpu}
              disabled={vulkanAvailable === false}
              onChange={(e) => set("useGpu", e.target.checked)}
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
            Speeds up Whisper when Vulkan works on this machine. If unavailable, transcription still
            runs on CPU.
          </p>
        </div>

        <div className="field">
          <span className="field-label">While processing</span>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={config.preventSleep}
              onChange={(e) => set("preventSleep", e.target.checked)}
            />
            <span>Prevent the computer from sleeping</span>
          </label>
          <p className="field-hint">
            Holds an idle-inhibit lock for the duration of a batch. Windows may still sleep on
            battery (Modern Standby). A failure is logged and the batch continues.
          </p>
        </div>

        <div className="field">
          <span className="field-label">Speakers</span>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={config.diarizationEnabled}
              onChange={(e) => set("diarizationEnabled", e.target.checked)}
            />
            <span>Label speakers in the transcript</span>
          </label>
          <label className="field-label field-follow" htmlFor="maxSpeakers">
            Exact speaker count (0 = auto)
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
          <p className="field-hint">
            Downloads two small ONNX models (~32 MB) on first use into{" "}
            <code>~/.cache/voxmd/diarize/</code>. Each transcript line becomes{" "}
            <code>[HH:MM:SS] **Speaker N:** …</code>. 0 lets clustering decide (at most{" "}
            {MAX_SPEAKERS} speakers). Set 2 for a two-person interview. If diarization fails, the
            unlabeled transcript is kept. Expect noticeably longer processing per file.
          </p>
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-section-title">Dictation</h2>
        <p className="field-hint">
          Live microphone transcription uses its own Whisper model so the batch queue can keep a
          larger one. Dictation and a running batch cannot overlap.
        </p>

        <div className="field">
          <label className="field-label" htmlFor="dmodel">
            Dictation model
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
            <option value={CUSTOM_MODEL}>Custom path…</option>
          </select>
          {showDictationCustom ? (
            <div className="input-with-button custom-model-row">
              <input
                className="input"
                placeholder="/absolute/path/to/model.bin"
                value={config.dictationModel}
                onChange={(e) => set("dictationModel", e.target.value)}
                aria-label="Path to a local dictation Whisper model"
              />
              <button
                type="button"
                className="btn-secondary btn-sm"
                title="Choose a local .bin or .gguf model file"
                onClick={onPickDictationModelFile}
              >
                <FolderOpen size={16} aria-hidden />
                <span>Choose…</span>
              </button>
            </div>
          ) : null}
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-section-title">Appearance</h2>
        <div className="lang-option-row">
          {(["system", "light", "dark"] as const).map((mode) => (
            <label className="lang-radio" key={mode}>
              <input
                type="radio"
                name="themeMode"
                checked={themeMode === mode}
                onChange={() => onThemeChange(mode)}
              />
              <span>{mode === "system" ? "System" : mode === "light" ? "Light" : "Dark"}</span>
            </label>
          ))}
        </div>
        <p className="field-hint shortcuts-hint">
          Shortcuts: F5 start / record, Esc cancel / stop, Ctrl+O files, Ctrl+, settings, Ctrl+1
          queue, Ctrl+2 dictation. Ignored while typing in a field.
        </p>
      </section>

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
              <Check size={15} aria-hidden /> Saved
            </>
          ) : saveState === "saving" ? (
            <>
              <Loader2 size={14} className="icon spin" aria-hidden /> Saving…
            </>
          ) : (
            "Save"
          )}
        </button>
        <button
          type="button"
          className="btn-secondary"
          title="Restore default settings (saved podcast feeds are kept)"
          onClick={onResetDefaults}
        >
          Reset defaults
        </button>
      </div>
    </Modal>
  );
}
