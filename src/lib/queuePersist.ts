import type { JobRow, QueueItem } from "../types";

function isEpisode(value: unknown): boolean {
  if (!value || typeof value !== "object") return false;
  const ep = value as Record<string, unknown>;
  return (
    typeof ep.feedTitle === "string" &&
    typeof ep.title === "string" &&
    typeof ep.outputDir === "string" &&
    ep.outputDir.length > 0
  );
}

/**
 * Keep unfinished (and failed) rows; drop completed exports.
 *
 * A `skipped` row means two different things: the Markdown already existed
 * (finished — nothing left to do), or the batch was cancelled before the item
 * ran (unfinished — restore it). `outputPath` is only set in the first case, so
 * an already-exported episode no longer comes back as "waiting" on every start.
 */
export function itemsToPersist(items: QueueItem[], jobs: Record<string, JobRow>): QueueItem[] {
  return items.filter((item) => {
    const job = jobs[item.id];
    if (!job) return true;
    if (job.stage === "done") return false;
    return !(job.stage === "skipped" && !!job.outputPath);
  });
}

export function isQueueItem(value: unknown): value is QueueItem {
  if (!value || typeof value !== "object") return false;
  const v = value as Partial<QueueItem>;
  if (
    typeof v.id !== "string" ||
    v.id.length === 0 ||
    (v.kind !== "local" && v.kind !== "podcast") ||
    typeof v.source !== "string" ||
    typeof v.displayName !== "string"
  ) {
    return false;
  }
  if (v.kind === "podcast") return isEpisode(v.episode);
  return true;
}

export function parseSavedQueue(raw: unknown): QueueItem[] {
  if (!Array.isArray(raw)) return [];
  const out: QueueItem[] = [];
  const seen = new Set<string>();
  for (const entry of raw) {
    if (!isQueueItem(entry) || seen.has(entry.id)) continue;
    seen.add(entry.id);
    out.push(entry);
  }
  return out;
}
