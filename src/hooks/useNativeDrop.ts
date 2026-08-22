import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import type { MessageKey } from "../i18n";
import { isAudioPath, localItem } from "../lib/queue";
import type { QueueItem } from "../types";

/**
 * The Tauri webview suppresses HTML5 drops and emits its own event carrying
 * real filesystem paths, so this cannot be done with DOM handlers.
 *
 * `t` is a parameter for the same reason as in `useBatchEvents`: this runs in
 * the component that provides the i18n context. It is read through a ref so a
 * language change does not tear down and re-register the drop listener.
 */
export function useNativeDrop(
  addItems: (items: QueueItem[]) => void,
  onStatus: (msg: string) => void,
  t: (key: MessageKey, params?: Record<string, string | number>) => string,
): boolean {
  const [dragActive, setDragActive] = useState(false);
  const tRef = useRef(t);
  useEffect(() => {
    tRef.current = t;
  }, [t]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    (async () => {
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        const kind = event.payload.type;
        if (kind === "enter" || kind === "over") {
          setDragActive(true);
          return;
        }
        if (kind === "leave") {
          setDragActive(false);
          return;
        }
        if (kind !== "drop") return;

        setDragActive(false);
        const paths = event.payload.paths;
        const audio = paths.filter(isAudioPath);
        if (audio.length > 0) addItems(audio.map(localItem));

        const ignored = paths.length - audio.length;
        if (ignored > 0) {
          onStatus(
            audio.length > 0
              ? tRef.current("msg.filesAddedIgnored", { added: audio.length, ignored })
              : tRef.current("msg.noSupportedFiles"),
          );
        } else if (audio.length > 0) {
          onStatus(tRef.current("msg.filesAdded", { count: audio.length }));
        }
      });
      if (cancelled) stop();
      else unlisten = stop;
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addItems, onStatus]);

  return dragActive;
}
