import type { MessageKey } from "../i18n";
import type { JobRow, QueueItem } from "../types";

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

/** Stages where the backend is actually working on the entry right now. */
export const ACTIVE_STAGES = ["download", "whisper", "diarize", "llm"] as const;
const ACTIVE = new Set<string>(ACTIVE_STAGES);

export interface QueueCounts {
  waiting: number;
  running: number;
  done: number;
  failed: number;
  total: number;
}

/**
 * What the queue currently holds, for the footer.
 *
 * Counted over the queue itself rather than over `jobs`, which also retains
 * rows for entries the user has since removed.
 */
export function queueCounts(items: QueueItem[], jobs: Record<string, JobRow>): QueueCounts {
  const counts: QueueCounts = { waiting: 0, running: 0, done: 0, failed: 0, total: items.length };
  for (const item of items) {
    const stage = jobs[item.id]?.stage ?? "queued";
    if (stage === "done") counts.done += 1;
    else if (stage === "error") counts.failed += 1;
    else if (ACTIVE.has(stage)) counts.running += 1;
    // `skipped` counts as neither: it was not processed and did not fail.
    else if (stage !== "skipped") counts.waiting += 1;
  }
  return counts;
}

/**
 * The entries the backend is working on, in queue order.
 *
 * There can be two: the pipeline runs Whisper and the LLM concurrently through
 * a channel of capacity one, so while file *n* is being summarised, file *n+1*
 * is already being transcribed. A single "current job" would be wrong.
 */
export function activeJobs(items: QueueItem[], jobs: Record<string, JobRow>): JobRow[] {
  return items
    .map((item) => jobs[item.id])
    .filter((job): job is JobRow => !!job && ACTIVE.has(job.stage));
}

/** Percentage for a row's own progress bar, or `null` when the stage has none. */
export function jobPercent(row: JobRow): number | null {
  if (row.stage === "download") return row.downloadPct ?? null;
  if (row.stage === "whisper") return row.whisperPct ?? null;
  // Diarization and the summary report stages, not percentages.
  return null;
}

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

/**
 * Human-readable byte size.
 *
 * Binary units, because that is what a file manager shows for a model on disk
 * and a mismatch between the two reads as a bug. One decimal from MB up; whole
 * numbers below, where a tenth of a kilobyte means nothing.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const decimals = unit >= 2 && value < 100 ? 1 : 0;
  return `${value.toFixed(decimals)} ${units[unit]}`;
}

export function toMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
