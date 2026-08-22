import type { MessageKey } from "../i18n";
import type { JobRow } from "../types";

type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

/**
 * Colour carries meaning: blue while work is in progress, green done, red
 * failed, orange skipped, grey waiting. Sharing one colour between an active
 * and a terminal stage made the status column unscannable.
 *
 * An unknown stage falls back to its raw name — a new backend stage should show
 * up as itself rather than disappear.
 */
export function badgeForStage(stage: string, t: Translate): { className: string; label: string } {
  switch (stage) {
    case "done":
      return { className: "badge-ok", label: t("stage.done") };
    case "error":
      return { className: "badge-error", label: t("stage.error") };
    case "skipped":
      return { className: "badge-warn", label: t("stage.skipped") };
    case "download":
      return { className: "badge-active", label: t("stage.download") };
    case "whisper":
      return { className: "badge-active", label: t("stage.whisper") };
    case "diarize":
      return { className: "badge-active", label: t("stage.diarize") };
    case "llm":
      return { className: "badge-active", label: t("stage.llm") };
    case "queued":
      return { className: "badge-neutral", label: t("stage.queued") };
    default:
      return { className: "badge-neutral", label: stage };
  }
}

/**
 * `row.message` comes from the backend and stays in English; only the stage
 * placeholders shown before a message arrives are translated.
 */
export function detailsForRow(row: JobRow, t: Translate): string {
  switch (row.stage) {
    case "queued":
      return t("details.queued");
    case "download":
      return row.downloadPct != null && row.downloadPct > 0
        ? t("details.downloadPct", { pct: row.downloadPct })
        : t("details.download");
    case "whisper":
      return row.whisperPct != null && row.whisperPct > 0
        ? t("details.whisperPct", { pct: row.whisperPct })
        : t("details.whisper");
    case "diarize":
      return row.message ?? t("details.diarize");
    case "llm":
      return row.message ?? t("details.llm");
    default:
      return row.message ?? "";
  }
}

/**
 * The Markdown file this row produced, if any.
 *
 * `outputPath` is a field on the event payload. Rows written by an older build
 * (restored from the store, or in flight across an update) only carry the
 * human-readable message, so the previous string parsing stays as a fallback.
 */
export function outputPathOf(row: JobRow): string | null {
  if (row.outputPath) return row.outputPath;
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

/** Stages a row can end on. Anything else means work is still in flight. */
export const TERMINAL_STAGES = new Set(["done", "error", "skipped"]);

/**
 * Gives every still-running row a terminal stage once the batch is over.
 *
 * The backend emits a terminal event per item on the paths it controls, but a
 * panic in the Whisper task takes the loop down with rows left on `whisper` or
 * `diarize` — and a row frozen on an active stage looks exactly like one that is
 * still working. After `batch_complete` nothing may claim to be running.
 *
 * `failure` is the batch-level error, verbatim from the backend; without one the
 * rows were simply never reached, which is not a failure of theirs.
 */
export function settleStrandedRows(
  jobs: Record<string, JobRow>,
  failure: string | undefined,
  stoppedMessage: string,
): { jobs: Record<string, JobRow>; stranded: JobRow[] } {
  const next: Record<string, JobRow> = {};
  const stranded: JobRow[] = [];
  for (const [id, row] of Object.entries(jobs)) {
    if (TERMINAL_STAGES.has(row.stage)) {
      next[id] = row;
      continue;
    }
    const settled: JobRow = {
      ...row,
      stage: failure ? "error" : "skipped",
      message: failure ?? stoppedMessage,
    };
    next[id] = settled;
    stranded.push(settled);
  }
  return { jobs: stranded.length ? next : jobs, stranded };
}

export function toMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
