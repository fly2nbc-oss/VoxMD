import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { format, messagesFor, type MessageKey } from "../i18n";
import { badgeForStage, detailsForRow, outputPathOf, settleStrandedRows } from "./jobs";
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
