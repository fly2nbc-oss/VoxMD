import { useEffect, useRef } from "react";

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable;
}

export interface HotkeyHandlers {
  onStartOrToggleRecord: () => void;
  onCancelOrStop: () => void;
  onPickFiles: () => void;
  onOpenSettings: () => void;
  onQueueMode: () => void;
  onDictationMode: () => void;
}

/** App-wide shortcuts. Modal Escape handlers run in capture first and win. */
export function useHotkeys(handlers: HotkeyHandlers, enabled = true) {
  const handlersRef = useRef(handlers);

  useEffect(() => {
    handlersRef.current = handlers;
  }, [handlers]);

  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.altKey || e.metaKey) return;
      if (isTypingTarget(e.target)) return;

      const h = handlersRef.current;
      const ctrl = e.ctrlKey;
      if (e.key === "F5") {
        e.preventDefault();
        h.onStartOrToggleRecord();
        return;
      }
      if (e.key === "Escape") {
        h.onCancelOrStop();
        return;
      }
      if (ctrl && e.key.toLowerCase() === "o" && !e.shiftKey) {
        e.preventDefault();
        h.onPickFiles();
        return;
      }
      if (ctrl && e.key === ",") {
        e.preventDefault();
        h.onOpenSettings();
        return;
      }
      if (ctrl && e.key === "1") {
        e.preventDefault();
        h.onQueueMode();
        return;
      }
      if (ctrl && e.key === "2") {
        e.preventDefault();
        h.onDictationMode();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [enabled]);
}
