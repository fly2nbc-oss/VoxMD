import { Check, FolderOpen, Loader2 } from "lucide-react";
import { isSummarySystemLanguage, isTranscriptionAuto } from "../lib/configStore";
import type { ThemeMode } from "../lib/theme";
import type { AppConfig, WhisperModelInfo } from "../types";
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
  onPickModelFile: () => void;

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
  onPickModelFile,
  detectedSystemSummaryLang,
  vulkanAvailable,
  themeMode,
  onThemeChange,
}: Props) {
  const set = <K extends keyof AppConfig>(key: K, value: AppConfig[K]) =>
    onConfigChange({ ...config, [key]: value });

  const hasApiKey = config.apiKey.trim() !== "";
  const isPreset = modelInfos.some((m) => m.name === config.whisperModel);
  // While the model list is loading everything looks "custom", which briefly
  // rendered the custom-path field containing the preset name.
  const showCustomPath = !modelsLoading && !isPreset;

  return (
    <Modal title="Settings" onClose={onClose} variant="drawer">
      <section className="settings-section">
        <h2 className="settings-section-title">Summary (LLM)</h2>
        <p className="field-hint">
          OpenAI-compatible chat API for the Markdown summary. Only used when Summary is enabled in
          the toolbar. Without a key, transcription still works; the summary section is skipped.
        </p>

        <div className="field">
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
            onChange={(e) => set("apiKey", e.target.value)}
          />
          {config.includeSummary && !hasApiKey ? (
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
            disabled={!config.includeSummary}
            onChange={(e) => set("apiBaseUrl", e.target.value)}
          />
          <p className="field-hint">
            Endpoint root, without <code>/v1/chat/completions</code>.
          </p>
        </div>

        <div className="field">
          <label className="field-label" htmlFor="model">
            Model
          </label>
          <input
            id="model"
            className="input"
            placeholder="deepseek-chat"
            value={config.apiModel}
            disabled={!config.includeSummary}
            onChange={(e) => set("apiModel", e.target.value)}
          />
          <p className="field-hint">Model id as expected by that provider.</p>
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
                onClick={onPickModelFile}
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
