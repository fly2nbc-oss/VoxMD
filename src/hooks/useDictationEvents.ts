import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import type { DictationStatusPayload } from "../types";

export interface DictationEvents {
  /** Backend stage: idle / loading / listening / finalizing / stopped / error. */
  stage: string;
  running: boolean;
  level: number;
  /** Text committed after a pause; `null` until the first commit arrives. */
  lastFinal: { text: string; seq: number } | null;
  partial: string;
  clearPartial: () => void;
}

const RUNNING_STAGES = new Set(["loading", "listening", "finalizing"]);

/**
 * Subscribes to the dictation events for the lifetime of the app.
 *
 * These listeners must not live in `DictationView`: leaving dictation mode
 * unmounts it, and the `stopped` event only arrives once the capture thread has
 * wound down. With the listener gone, `running` stayed true forever, which
 * blocked the batch Start button with "Stop dictation first" and left no way
 * out but restarting the app.
 */
export function useDictationEvents(onStatus: (message: string) => void): DictationEvents {
  const [stage, setStage] = useState("idle");
  const [running, setRunning] = useState(false);
  const [level, setLevel] = useState(0);
  const [partial, setPartial] = useState("");
  const [lastFinal, setLastFinal] = useState<{ text: string; seq: number } | null>(null);
  const seqRef = useRef(0);
  const statusRef = useRef(onStatus);

  useEffect(() => {
    statusRef.current = onStatus;
  }, [onStatus]);

  // The backend owns the running state; recover it after a webview reload.
  useEffect(() => {
    void invoke<boolean>("dictation_state")
      .then(setRunning)
      .catch(() => {});
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const track = (stop: () => void) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    };

    void (async () => {
      track(
        await listen<DictationStatusPayload>("dictation_status", (e) => {
          const { stage: next, message } = e.payload;
          setStage(next);
          if (message) statusRef.current(message);
          if (next === "stopped" || next === "error") {
            setRunning(false);
            setPartial("");
          } else if (RUNNING_STAGES.has(next)) {
            setRunning(true);
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
          setPartial("");
          if (!text) return;
          seqRef.current += 1;
          setLastFinal({ text, seq: seqRef.current });
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
  }, []);

  return { stage, running, level, lastFinal, partial, clearPartial: () => setPartial("") };
}
