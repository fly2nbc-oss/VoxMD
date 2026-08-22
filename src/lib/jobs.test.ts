import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { format, messagesFor, type MessageKey } from "../i18n";
import {
  activeJobs,
  badgeForStage,
  detailsForRow,
  jobPercent,
  outputPathOf,
  queueCounts,
  settleStrandedRows,
} from "./jobs";
import type { QueueItem } from "../types";
import type { JobRow } from "../types";

const t = (key: MessageKey, params?: Record<string, string | number>) =>
  format(en, key, params);
const tDe = (key: MessageKey, params?: Record<string, string | number>) =>
  format(messagesFor("de"), key, params);

function row(partial: Partial<JobRow> & Pick<JobRow, "stage">): JobRow {
  return {
    path: "/a.mp3",
    displayName: "a.mp3",
    ...partial,
  };
}

describe("outputPathOf", () => {
  it("prefers the payload field over the message", () => {
    expect(
      outputPathOf(
        row({ stage: "done", outputPath: "/out/Talk.md", message: "Saved: /out/Talk.md" }),
      ),
    ).toBe("/out/Talk.md");
    // A path containing the deletion-note wording would defeat the regex.
    expect(
      outputPathOf(
        row({
          stage: "done",
          outputPath: "/out/a (audio deletion failed: x).md",
          message: "Saved: /out/a (audio deletion failed: x).md",
        }),
      ),
    ).toBe("/out/a (audio deletion failed: x).md");
  });

  it("falls back to parsing rows from an older build", () => {
    expect(
      outputPathOf(row({ stage: "done", message: "Saved: /out/Talk.md" })),
    ).toBe("/out/Talk.md");
    expect(
      outputPathOf(
        row({
          stage: "done",
          message: "Saved: /out/Talk.md (audio deletion failed: permission denied)",
        }),
      ),
    ).toBe("/out/Talk.md");
  });

  it("parses Skipped (exists) paths", () => {
    expect(
      outputPathOf(row({ stage: "skipped", message: "Skipped (exists): /out/Talk.md" })),
    ).toBe("/out/Talk.md");
  });

  it("returns null when there is nothing to open", () => {
    expect(outputPathOf(row({ stage: "done" }))).toBeNull();
    expect(outputPathOf(row({ stage: "whisper", message: "Transcribing…" }))).toBeNull();
    expect(outputPathOf(row({ stage: "skipped", message: "Cancelled." }))).toBeNull();
  });
});

describe("badgeForStage", () => {
  it("maps known stages", () => {
    expect(badgeForStage("done", t).label).toBe("Done");
    expect(badgeForStage("error", t).className).toBe("badge-error");
    expect(badgeForStage("whisper", t).label).toBe("Whisper");
    expect(badgeForStage("diarize", t).label).toBe("Speakers");
    expect(badgeForStage("diarize", t).className).toBe("badge-active");
    expect(badgeForStage("diarize", tDe).label).toBe("Sprecher");
    // An unknown stage keeps its raw name rather than vanishing.
    expect(badgeForStage("brand-new", t).label).toBe("brand-new");
  });
});

describe("detailsForRow", () => {
  it("includes download and whisper percentages", () => {
    expect(detailsForRow(row({ stage: "download", downloadPct: 40 }), t)).toContain("40%");
    expect(detailsForRow(row({ stage: "whisper", whisperPct: 12 }), t)).toContain("12%");
    expect(detailsForRow(row({ stage: "queued" }), t)).toContain("Waiting");
    expect(detailsForRow(row({ stage: "diarize" }), t)).toContain("Separating");
  });

  it("translates the placeholders but leaves backend messages alone", () => {
    expect(detailsForRow(row({ stage: "queued" }), tDe)).toBe("Wartet in der Warteschlange…");
    expect(detailsForRow(row({ stage: "whisper", whisperPct: 12 }), tDe)).toContain("12");
    // `message` is produced by the Rust side and crosses IPC as free text.
    expect(detailsForRow(row({ stage: "llm", message: "Summary… (part 2/3)" }), tDe)).toBe(
      "Summary… (part 2/3)",
    );
  });
});

describe("settleStrandedRows", () => {
  const rows = (...specs: Array<[string, string]>): Record<string, JobRow> =>
    Object.fromEntries(
      specs.map(([path, stage]) => [path, { path, displayName: path, stage }]),
    );

  it("leaves a fully settled batch untouched, by identity", () => {
    const before = rows(["a", "done"], ["b", "error"], ["c", "skipped"]);
    const after = settleStrandedRows(before, undefined, "stopped");
    expect(after.jobs).toBe(before);
    expect(after.stranded).toEqual([]);
  });

  /** The reported bug: a panic left a row on "diarize" forever. */
  it("turns a row frozen mid-stage into a failure carrying the batch error", () => {
    const before = rows(["a", "done"], ["b", "diarize"], ["c", "queued"]);
    const { jobs, stranded } = settleStrandedRows(before, "task 65 panicked", "stopped");
    expect(jobs.a.stage).toBe("done");
    expect(jobs.b).toMatchObject({ stage: "error", message: "task 65 panicked" });
    expect(jobs.c).toMatchObject({ stage: "error", message: "task 65 panicked" });
    expect(stranded.map((r) => r.path)).toEqual(["b", "c"]);
  });

  it("marks them skipped, not failed, when the batch ended without an error", () => {
    const { jobs, stranded } = settleStrandedRows(rows(["b", "whisper"]), undefined, "stopped");
    expect(jobs.b).toMatchObject({ stage: "skipped", message: "stopped" });
    // A cancel is not the row's failure, so it stays out of the error panel.
    expect(stranded).toHaveLength(1);
  });

  it("covers every non-terminal stage the backend can emit", () => {
    const active = ["queued", "download", "whisper", "diarize", "llm"];
    const { jobs } = settleStrandedRows(
      rows(...active.map((s) => [s, s] as [string, string])),
      "boom",
      "stopped",
    );
    for (const stage of active) expect(jobs[stage].stage, stage).toBe("error");
  });
});

describe("queueCounts", () => {
  const item = (id: string): QueueItem => ({ id, kind: "local", source: id, displayName: id });
  const items = ["a", "b", "c", "d", "e", "f"].map(item);
  const jobs = (spec: Record<string, string>): Record<string, JobRow> =>
    Object.fromEntries(
      Object.entries(spec).map(([id, stage]) => [id, { path: id, displayName: id, stage }]),
    );

  it("counts each bucket and treats an unknown entry as waiting", () => {
    const c = queueCounts(
      items,
      jobs({ a: "done", b: "error", c: "whisper", d: "llm", e: "queued" }),
    );
    // `f` has no job row at all — it has not started, so it waits.
    expect(c).toEqual({ waiting: 2, running: 2, done: 1, failed: 1, total: 6 });
  });

  it("counts a skipped entry as neither done nor failed", () => {
    const c = queueCounts([item("a")], jobs({ a: "skipped" }));
    expect(c).toEqual({ waiting: 0, running: 0, done: 0, failed: 0, total: 1 });
  });

  it("counts over the queue, not over stale job rows", () => {
    // `jobs` keeps rows for entries the user removed; they must not be counted.
    const c = queueCounts([item("a")], jobs({ a: "queued", removed: "done" }));
    expect(c.total).toBe(1);
    expect(c.done).toBe(0);
  });
});

describe("activeJobs", () => {
  const item = (id: string): QueueItem => ({ id, kind: "local", source: id, displayName: id });

  /** The pipeline transcribes one file while summarising the previous one. */
  it("returns both concurrent stages, in queue order", () => {
    const items = [item("a"), item("b"), item("c")];
    const jobs: Record<string, JobRow> = {
      a: { path: "a", displayName: "a", stage: "llm" },
      b: { path: "b", displayName: "b", stage: "diarize" },
      c: { path: "c", displayName: "c", stage: "queued" },
    };
    expect(activeJobs(items, jobs).map((j) => j.path)).toEqual(["a", "b"]);
  });

  it("is empty when nothing is running", () => {
    const items = [item("a")];
    expect(activeJobs(items, { a: { path: "a", displayName: "a", stage: "done" } })).toEqual([]);
    expect(activeJobs(items, {})).toEqual([]);
  });
});

describe("jobPercent", () => {
  it("reports a percentage only for the stages that measure one", () => {
    expect(jobPercent(row({ stage: "download", downloadPct: 40 }))).toBe(40);
    expect(jobPercent(row({ stage: "whisper", whisperPct: 12 }))).toBe(12);
    // Diarization and the summary report a stage, not a number — a bar here
    // would be a fabricated one.
    expect(jobPercent(row({ stage: "diarize" }))).toBeNull();
    expect(jobPercent(row({ stage: "llm" }))).toBeNull();
    expect(jobPercent(row({ stage: "whisper" }))).toBeNull();
  });
});
