import { invoke } from "@tauri-apps/api/core";
import { Check, Copy, Eraser, Languages, Loader2, Mic, MicOff, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DictationEvents } from "../hooks/useDictationEvents";
import { useT } from "../i18n/I18nProvider";
import { toMsg } from "../lib/jobs";
import type { AppConfig, MicrophoneInfo } from "../types";

/**
 * `value` goes into the prompt, so it stays the English language name the model
 * understands; `label` is what the user reads. `Intl.DisplayNames` renders the
 * label in the current UI language, and falls back to the English name where a
 * platform has no data for it.
 */
const TRANSLATE_TARGETS: Array<{ value: string; iso: string }> = [
  { value: "German", iso: "de" },
  { value: "English", iso: "en" },
  { value: "French", iso: "fr" },
  { value: "Spanish", iso: "es" },
  { value: "Italian", iso: "it" },
  { value: "Portuguese", iso: "pt" },
  { value: "Dutch", iso: "nl" },
  { value: "Polish", iso: "pl" },
  { value: "Japanese", iso: "ja" },
  { value: "Chinese", iso: "zh" },
];

function targetLabel(iso: string, fallback: string, uiLang: string): string {
  try {
    return new Intl.DisplayNames([uiLang], { type: "language" }).of(iso) ?? fallback;
  } catch {
    return fallback;
  }
}

interface Props {
  config: AppConfig;
  storeReady: boolean;
  processing: boolean;
  /** Live dictation state, owned by the app so it survives a mode switch. */
  dictation: DictationEvents;
  onStart: () => void;
  onStop: () => void;
  onMicrophoneChange: (name: string) => void;
  onStatus: (message: string) => void;
}

export function DictationView({
  config,
  storeReady,
  processing,
  dictation,
  onStart,
  onStop,
  onMicrophoneChange,
  onStatus,
}: Props) {
  const { t, lang } = useT();
  const { stage, running, level, partial, lastFinal, clearPartial } = dictation;
  const [committed, setCommitted] = useState("");
  const [proposal, setProposal] = useState<string | null>(null);
  const [mics, setMics] = useState<MicrophoneInfo[]>([]);
  const lastAppliedSeq = useRef(0);
  const [aiBusy, setAiBusy] = useState<"improve" | "translate" | null>(null);
  const [translateTarget, setTranslateTarget] = useState("English");
  const [copied, setCopied] = useState(false);

  const hasApiKey = config.apiKey.trim() !== "";
  const displayText = useMemo(() => {
    const head = committed.trimEnd();
    if (!partial.trim()) return head;
    return head ? `${head} ${partial.trim()}` : partial.trim();
  }, [committed, partial]);

  useEffect(() => {
    let cancelled = false;
    void invoke<MicrophoneInfo[]>("list_microphones")
      .then((list) => {
        if (!cancelled) setMics(list);
      })
      .catch((e) => {
        if (!cancelled) onStatus(t("dictation.micsUnavailable", { error: toMsg(e) }));
      });
    return () => {
      cancelled = true;
    };
  }, [onStatus, t]);

  const captureOwnsMic = stage === "listening" || stage === "finalizing";

  useEffect(() => {
    if (captureOwnsMic || processing) return;
    let cancelled = false;
    void invoke("start_mic_monitor", { microphoneName: config.microphoneName }).catch((e) => {
      if (!cancelled) onStatus(t("dictation.micError", { error: toMsg(e) }));
    });
    return () => {
      cancelled = true;
      void invoke("stop_mic_monitor");
    };
  }, [captureOwnsMic, processing, config.microphoneName, onStatus, t]);

  // `seq` guards against re-appending the same commit when the view remounts
  // after a mode switch, since the hook keeps the last value around.
  useEffect(() => {
    if (!lastFinal || lastFinal.seq === lastAppliedSeq.current) return;
    lastAppliedSeq.current = lastFinal.seq;
    setCommitted((prev) => (prev.trim() ? `${prev.trimEnd()} ${lastFinal.text}` : lastFinal.text));
  }, [lastFinal]);

  const meterPct = Math.min(100, Math.round(Math.sqrt(Math.max(0, level)) * 280));

  const runAi = async (kind: "improve" | "translate") => {
    const text = displayText.trim();
    if (!text || !hasApiKey) return;
    setAiBusy(kind);
    try {
      const next =
        kind === "improve"
          ? await invoke<string>("improve_text", { config, text })
          : await invoke<string>("translate_text", {
              config,
              text,
              target: translateTarget,
            });
      setProposal(next);
    } catch (e) {
      onStatus(toMsg(e));
    } finally {
      setAiBusy(null);
    }
  };

  const copyAll = async () => {
    try {
      await navigator.clipboard.writeText(displayText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch (e) {
      onStatus(t("dictation.copyFailed", { error: toMsg(e) }));
    }
  };

  const startBlocked = !storeReady || processing || running;
  const startTitle = processing
    ? t("dictation.blockedByBatch")
    : running
      ? t("dictation.alreadyListening")
      : t("dictation.recordTitle");

  return (
    <div className="dictation">
      <div className="dictation-toolbar">
        <label className="dictation-mic">
          <span className="field-label">{t("dictation.microphone")}</span>
          <select
            className="input"
            value={config.microphoneName}
            disabled={running}
            onChange={(e) => onMicrophoneChange(e.target.value)}
            aria-label={t("dictation.microphone")}
          >
            <option value="">{t("dictation.systemDefault")}</option>
            {mics.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
                {m.isDefault ? ` ${t("dictation.isDefault")}` : ""}
              </option>
            ))}
          </select>
        </label>

        <div className="dictation-meter-wrap" title={t("dictation.inputLevel")}>
          <Mic
            size={16}
            className={meterPct > 6 ? "dictation-mic-live" : "dictation-mic-idle"}
            aria-hidden
          />
          <div className="dictation-meter" aria-hidden>
            <div className="dictation-meter-fill" style={{ width: `${meterPct}%` }} />
          </div>
        </div>

        {running ? (
          <button
            type="button"
            className="btn-secondary btn-sm"
            onClick={onStop}
            title={t("dictation.stopTitle")}
          >
            <MicOff size={16} aria-hidden />
            <span>{t("dictation.stop")}</span>
          </button>
        ) : (
          <button
            type="button"
            className="btn-primary btn-sm"
            disabled={startBlocked}
            onClick={onStart}
            title={startTitle}
          >
            <Mic size={16} aria-hidden />
            <span>{t("dictation.record")}</span>
          </button>
        )}
      </div>

      <p className="field-hint dictation-stage">
        {stage === "listening"
          ? t("dictation.stageListening")
          : stage === "loading"
            ? t("dictation.stageLoading")
            : stage === "finalizing"
              ? t("dictation.stageFinalizing")
              : t("dictation.stageIdle")}
      </p>

      <div className="dictation-editor">
        <textarea
          className="input dictation-committed"
          value={committed}
          onChange={(e) => setCommitted(e.target.value)}
          placeholder={t("dictation.placeholder")}
          aria-label={t("dictation.transcriptAria")}
          spellCheck
        />
        {partial.trim() ? (
          <p className="dictation-partial" aria-live="polite">
            {partial.trim()}
          </p>
        ) : null}
      </div>

      {proposal !== null ? (
        <div className="dictation-proposal">
          <p className="field-label">{t("dictation.suggestion")}</p>
          <textarea className="input dictation-proposal-text" value={proposal} readOnly />
          <div className="dictation-proposal-actions">
            <button
              type="button"
              className="btn-primary btn-sm"
              onClick={() => {
                setCommitted(proposal);
                clearPartial();
                setProposal(null);
              }}
            >
              <Check size={14} aria-hidden />
              {t("dictation.keep")}
            </button>
            <button type="button" className="btn-secondary btn-sm" onClick={() => setProposal(null)}>
              <X size={14} aria-hidden />
              {t("dictation.discard")}
            </button>
          </div>
        </div>
      ) : null}

      <div className="dictation-actions">
        <button
          type="button"
          className="btn-secondary btn-sm"
          disabled={!displayText}
          onClick={() => void copyAll()}
        >
          {copied ? <Check size={14} aria-hidden /> : <Copy size={14} aria-hidden />}
          <span>{copied ? t("dictation.copied") : t("dictation.copy")}</span>
        </button>
        <button
          type="button"
          className="btn-secondary btn-sm"
          disabled={!committed && !partial}
          onClick={() => {
            setCommitted("");
            clearPartial();
            setProposal(null);
          }}
        >
          <Eraser size={14} aria-hidden />
          <span>{t("dictation.clear")}</span>
        </button>

        {hasApiKey ? (
          <>
            <button
              type="button"
              className="btn-secondary btn-sm"
              disabled={!displayText.trim() || aiBusy !== null || running}
              onClick={() => void runAi("improve")}
              title={t("dictation.improveTitle")}
            >
              {aiBusy === "improve" ? (
                <Loader2 size={14} className="icon spin" aria-hidden />
              ) : (
                <Sparkles size={14} aria-hidden />
              )}
              <span>{t("dictation.improve")}</span>
            </button>
            <label className="dictation-translate">
              <select
                className="input"
                value={translateTarget}
                disabled={aiBusy !== null}
                onChange={(e) => setTranslateTarget(e.target.value)}
                aria-label={t("dictation.translateInto")}
              >
                {TRANSLATE_TARGETS.map((target) => (
                  <option key={target.value} value={target.value}>
                    {targetLabel(target.iso, target.value, lang)}
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              className="btn-secondary btn-sm"
              disabled={!displayText.trim() || aiBusy !== null || running}
              onClick={() => void runAi("translate")}
            >
              {aiBusy === "translate" ? (
                <Loader2 size={14} className="icon spin" aria-hidden />
              ) : (
                <Languages size={14} aria-hidden />
              )}
              <span>{t("dictation.translate")}</span>
            </button>
          </>
        ) : (
          <p className="field-hint">{t("dictation.needKey")}</p>
        )}
      </div>
    </div>
  );
}
