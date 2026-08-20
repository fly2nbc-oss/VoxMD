import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Copy, Eraser, Loader2, Mic, MicOff, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { toMsg } from "../lib/jobs";
import type { AppConfig, DictationStatusPayload, MicrophoneInfo } from "../types";

const TRANSLATE_TARGETS = [
  "German",
  "English",
  "French",
  "Spanish",
  "Italian",
  "Portuguese",
  "Dutch",
  "Polish",
  "Japanese",
  "Chinese",
];

interface Props {
  config: AppConfig;
  storeReady: boolean;
  processing: boolean;
  running: boolean;
  onRunningChange: (running: boolean) => void;
  onStart: () => void;
  onStop: () => void;
  onMicrophoneChange: (name: string) => void;
  onStatus: (message: string) => void;
}

export function DictationView({
  config,
  storeReady,
  processing,
  running,
  onRunningChange,
  onStart,
  onStop,
  onMicrophoneChange,
  onStatus,
}: Props) {
  const [committed, setCommitted] = useState("");
  const [partial, setPartial] = useState("");
  const [proposal, setProposal] = useState<string | null>(null);
  const [level, setLevel] = useState(0);
  const [stage, setStage] = useState("idle");
  const [mics, setMics] = useState<MicrophoneInfo[]>([]);
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
        if (!cancelled) onStatus(`Microphones unavailable: ${toMsg(e)}`);
      });
    return () => {
      cancelled = true;
    };
  }, [onStatus]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const track = (stop: () => void) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    };

    (async () => {
      track(
        await listen<DictationStatusPayload>("dictation_status", (e) => {
          const { stage: next, message } = e.payload;
          setStage(next);
          if (message) onStatus(message);
          if (next === "stopped" || next === "error") {
            onRunningChange(false);
            setPartial("");
          }
          if (next === "listening" || next === "loading" || next === "finalizing") {
            onRunningChange(true);
          }
        }),
      );
      track(
        await listen<{ text: string }>("dictation_partial", (e) => {
          setPartial(e.payload.text ?? "");
        }),
      );
      track(
        await listen<{ text: string }>("dictation_final", (e) => {
          const text = (e.payload.text ?? "").trim();
          if (!text) {
            setPartial("");
            return;
          }
          setCommitted((prev) => (prev.trim() ? `${prev.trimEnd()} ${text}` : text));
          setPartial("");
        }),
      );
      track(
        await listen<{ rms: number }>("dictation_level", (e) => {
          setLevel(e.payload.rms ?? 0);
        }),
      );
    })();

    return () => {
      cancelled = true;
      for (const stop of unlisteners) stop();
    };
  }, [onRunningChange, onStatus]);

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
      onStatus(`Could not copy: ${toMsg(e)}`);
    }
  };

  const startBlocked = !storeReady || processing || running;
  const startTitle = processing
    ? "Stop the batch before dictating"
    : running
      ? "Already listening"
      : "Start dictation (F5)";

  return (
    <div className="dictation">
      <div className="dictation-toolbar">
        <label className="dictation-mic">
          <span className="field-label">Microphone</span>
          <select
            className="input"
            value={config.microphoneName}
            disabled={running}
            onChange={(e) => onMicrophoneChange(e.target.value)}
            aria-label="Microphone"
          >
            <option value="">System default</option>
            {mics.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
                {m.isDefault ? " (default)" : ""}
              </option>
            ))}
          </select>
        </label>

        <div className="dictation-meter" title="Input level" aria-hidden>
          <div className="dictation-meter-fill" style={{ width: `${meterPct}%` }} />
        </div>

        {running ? (
          <button
            type="button"
            className="btn-secondary btn-sm"
            onClick={onStop}
            title="Stop dictation (Esc)"
          >
            <MicOff size={16} aria-hidden />
            <span>Stop</span>
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
            <span>Record</span>
          </button>
        )}
      </div>

      <p className="field-hint dictation-stage">
        {stage === "listening"
          ? "Listening — pause to commit a phrase."
          : stage === "loading"
            ? "Loading Whisper model…"
            : stage === "finalizing"
              ? "Finalizing…"
              : "Idle. Press Record (F5) and speak."}
      </p>

      <div className="dictation-editor">
        <textarea
          className="input dictation-committed"
          value={committed}
          onChange={(e) => setCommitted(e.target.value)}
          placeholder="Transcript appears here…"
          aria-label="Dictation transcript"
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
          <p className="field-label">Suggestion</p>
          <textarea className="input dictation-proposal-text" value={proposal} readOnly />
          <div className="dictation-proposal-actions">
            <button
              type="button"
              className="btn-primary btn-sm"
              onClick={() => {
                setCommitted(proposal);
                setPartial("");
                setProposal(null);
              }}
            >
              <Check size={14} aria-hidden />
              Keep
            </button>
            <button type="button" className="btn-secondary btn-sm" onClick={() => setProposal(null)}>
              <X size={14} aria-hidden />
              Discard
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
          <span>{copied ? "Copied" : "Copy"}</span>
        </button>
        <button
          type="button"
          className="btn-secondary btn-sm"
          disabled={!committed && !partial}
          onClick={() => {
            setCommitted("");
            setPartial("");
            setProposal(null);
          }}
        >
          <Eraser size={14} aria-hidden />
          <span>Clear</span>
        </button>

        {hasApiKey ? (
          <>
            <button
              type="button"
              className="btn-secondary btn-sm"
              disabled={!displayText.trim() || aiBusy !== null || running}
              onClick={() => void runAi("improve")}
              title="Rewrite the transcript with the configured LLM"
            >
              {aiBusy === "improve" ? (
                <Loader2 size={14} className="icon spin" aria-hidden />
              ) : (
                <Sparkles size={14} aria-hidden />
              )}
              <span>Improve</span>
            </button>
            <label className="dictation-translate">
              <select
                className="input"
                value={translateTarget}
                disabled={aiBusy !== null}
                onChange={(e) => setTranslateTarget(e.target.value)}
                aria-label="Translate into"
              >
                {TRANSLATE_TARGETS.map((t) => (
                  <option key={t} value={t}>
                    {t}
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
              ) : null}
              <span>Translate</span>
            </button>
          </>
        ) : (
          <p className="field-hint">Add an API key in Settings to improve or translate.</p>
        )}
      </div>
    </div>
  );
}
