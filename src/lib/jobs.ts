import type { JobRow } from "../types";

/**
 * Colour carries meaning: blue while work is in progress, green done, red
 * failed, orange skipped, grey waiting. Sharing one colour between an active
 * and a terminal stage made the status column unscannable.
 */
export function badgeForStage(stage: string): { className: string; label: string } {
  switch (stage) {
    case "done":
      return { className: "badge-ok", label: "Done" };
    case "error":
      return { className: "badge-error", label: "Error" };
    case "skipped":
      return { className: "badge-warn", label: "Skipped" };
    case "download":
      return { className: "badge-active", label: "Download" };
    case "whisper":
      return { className: "badge-active", label: "Whisper" };
    case "llm":
      return { className: "badge-active", label: "LLM" };
    case "queued":
      return { className: "badge-neutral", label: "Wait" };
    default:
      return { className: "badge-neutral", label: stage };
  }
}

export function detailsForRow(row: JobRow): string {
  switch (row.stage) {
    case "queued":
      return "Waiting in queue…";
    case "download":
      return row.downloadPct != null && row.downloadPct > 0
        ? `Downloading episode… ${row.downloadPct}%`
        : "Downloading episode…";
    case "whisper":
      return row.whisperPct != null && row.whisperPct > 0
        ? `Transcribing… ${row.whisperPct}%`
        : "Transcribing…";
    case "llm":
      return row.message ?? "Summary…";
    default:
      return row.message ?? "";
  }
}

/**
 * Recovers the written path from the backend's status message so the row can
 * offer to open it.
 *
 * This parses a human-readable string, which is fragile: `pipeline.rs` produces
 * `Saved: <path>` and `Skipped (exists): <path>`, and changing either wording
 * silently disables the open buttons. A dedicated field on the event payload
 * would be sturdier if this needs to grow.
 */
export function outputPathOf(row: JobRow): string | null {
  const msg = row.message;
  if (!msg) return null;
  if (row.stage === "done") {
    // A trailing note such as " (audio deletion failed: …)" is not part of the path.
    return msg.replace(/^Saved:\s*/, "").replace(/\s+\(audio deletion failed:.*$/, "") || null;
  }
  if (row.stage === "skipped" && msg.startsWith("Skipped (exists): ")) {
    return msg.slice("Skipped (exists): ".length) || null;
  }
  return null;
}

export function toMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
