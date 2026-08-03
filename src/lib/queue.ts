import type { QueueItem } from "../types";

/**
 * Formats accepted for the file picker and drag-and-drop.
 *
 * Mirrors `AUDIO_EXTENSIONS` in `src-tauri/src/meta.rs`; a Rust test asserts the
 * two lists stay identical, since the backend uses its copy to decide which feed
 * enclosures count as audio.
 */
export const AUDIO_EXTENSIONS = ["mp3", "m4a", "mp4", "wav", "ogg", "flac", "webm", "opus"];

export function isAudioPath(p: string): boolean {
  const ext = p.split(".").pop()?.toLowerCase() ?? "";
  return AUDIO_EXTENSIONS.includes(ext);
}

export function localItem(path: string): QueueItem {
  return {
    id: path,
    kind: "local",
    source: path,
    displayName: path.split(/[/\\]/).pop() ?? path,
  };
}
