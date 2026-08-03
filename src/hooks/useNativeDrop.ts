import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";
import { isAudioPath, localItem } from "../lib/queue";
import type { QueueItem } from "../types";

/**
 * The Tauri webview suppresses HTML5 drops and emits its own event carrying
 * real filesystem paths, so this cannot be done with DOM handlers.
 */
export function useNativeDrop(
  addItems: (items: QueueItem[]) => void,
  onStatus: (msg: string) => void,
): boolean {
  const [dragActive, setDragActive] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    (async () => {
      const stop = await getCurrentWebview().onDragDropEvent((event) => {
        const t = event.payload.type;
        if (t === "enter" || t === "over") {
          setDragActive(true);
          return;
        }
        if (t === "leave") {
          setDragActive(false);
          return;
        }
        if (t !== "drop") return;

        setDragActive(false);
        const paths = event.payload.paths;
        const audio = paths.filter(isAudioPath);
        if (audio.length > 0) addItems(audio.map(localItem));

        const ignored = paths.length - audio.length;
        if (ignored > 0) {
          onStatus(
            audio.length > 0
              ? `${audio.length} file(s) added, ${ignored} unsupported item(s) ignored.`
              : "No supported audio files in the drop.",
          );
        } else if (audio.length > 0) {
          onStatus(`${audio.length} file(s) added.`);
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
