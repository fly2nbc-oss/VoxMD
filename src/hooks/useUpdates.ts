import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ASK_DELAY_MS,
  CHECK_DELAY_MS,
  defaultUpdateState,
  due,
  isNewer,
  normalizeUpdateState,
  statusFor,
  type UpdateState,
  type UpdateStatus,
} from "../lib/update";

export interface Updates {
  version: string;
  enabled: boolean;
  status: UpdateStatus;
  /** The latest version the last check found. */
  latest: string | null;
  /** When the last check got an answer. */
  checkedMs: number | null;
  /** First-start question is open. */
  asking: boolean;
  /** A newer version whose hint is on screen. */
  notice: string | null;
  setEnabled: (on: boolean) => void;
  answer: (yes: boolean) => void;
  checkNow: () => void;
  dismissNotice: () => void;
}

/**
 * Opt-in update check (R12: no dialog at start). The question comes once, when
 * the window has stood still for a moment and nothing else is open; the check
 * runs 10 s after the start and at most once a day; the hint appears once per
 * newer version. See `update.rs` for what actually goes over the wire.
 */
export function useUpdates(
  ready: boolean,
  /** Something else is open (Settings, Podcast): the question waits. */
  busy: boolean,
  load: () => Promise<unknown>,
  save: (state: UpdateState) => Promise<void>,
): Updates {
  const [state, setState] = useState<UpdateState>(defaultUpdateState);
  const [loaded, setLoaded] = useState(false);
  const [version, setVersion] = useState("");
  const [status, setStatus] = useState<UpdateStatus>("never");
  const [asking, setAsking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const stateRef = useRef(state);
  const checkingRef = useRef(false);
  /** This session's automatic check was scheduled (it runs once a session at most). */
  const scheduledRef = useRef(false);

  const commit = useCallback(
    (next: UpdateState) => {
      stateRef.current = next;
      setState(next);
      void save(next).catch(() => {
        /* non-fatal: the choice just will not persist */
      });
    },
    [save],
  );

  useEffect(() => {
    if (!ready) return;
    let cancelled = false;
    void (async () => {
      const [stored, current] = await Promise.all([
        load().catch(() => null),
        getVersion().catch(() => ""),
      ]);
      if (cancelled) return;
      const next = normalizeUpdateState(stored);
      stateRef.current = next;
      setState(next);
      setVersion(current);
      setStatus(statusFor(next, current));
      setLoaded(true);
    })();
    return () => {
      cancelled = true;
    };
  }, [ready, load]);

  const run = useCallback(
    async (manual: boolean) => {
      if (checkingRef.current || !version) return;
      checkingRef.current = true;
      setStatus("checking");
      try {
        const latest = await invoke<string>("check_for_update");
        const newer = isNewer(latest, version);
        const next: UpdateState = { ...stateRef.current, checkedMs: Date.now(), latest };
        if (newer && !manual && next.told !== latest) {
          next.told = latest;
          setNotice(latest);
        }
        commit(next);
        setStatus(newer ? "newer" : "current");
      } catch {
        setStatus("failed");
      } finally {
        checkingRef.current = false;
      }
    },
    [version, commit],
  );

  // The question: once, and only when nothing else is open.
  useEffect(() => {
    if (!loaded || state.asked || busy) return;
    const timer = window.setTimeout(() => setAsking(true), ASK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [loaded, state.asked, busy]);

  // The automatic check: switched on, due, once per session, after the start has settled.
  useEffect(() => {
    if (!loaded || !version || scheduledRef.current) return;
    if (!due(state.enabled, state.checkedMs, Date.now())) return;
    scheduledRef.current = true;
    const timer = window.setTimeout(() => void run(false), CHECK_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
      scheduledRef.current = false;
    };
  }, [loaded, version, state.enabled, state.checkedMs, run]);

  const setEnabled = useCallback(
    (on: boolean) => {
      commit({ ...stateRef.current, enabled: on, asked: true });
      setAsking(false);
    },
    [commit],
  );

  return {
    version,
    enabled: state.enabled,
    status,
    latest: state.latest,
    checkedMs: state.checkedMs,
    asking,
    notice,
    setEnabled,
    answer: setEnabled,
    checkNow: () => void run(true),
    dismissNotice: () => setNotice(null),
  };
}
