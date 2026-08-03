import { describe, expect, it } from "vitest";
import { badgeForStage, detailsForRow, outputPathOf } from "./jobs";
import type { JobRow } from "../types";

function row(partial: Partial<JobRow> & Pick<JobRow, "stage">): JobRow {
  return {
    path: "/a.mp3",
    displayName: "a.mp3",
    ...partial,
  };
}

describe("outputPathOf", () => {
  it("parses Saved paths and strips audio-deletion notes", () => {
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
    expect(badgeForStage("done").label).toBe("Done");
    expect(badgeForStage("error").className).toBe("badge-error");
    expect(badgeForStage("whisper").label).toBe("Whisper");
  });
});

describe("detailsForRow", () => {
  it("includes download and whisper percentages", () => {
    expect(detailsForRow(row({ stage: "download", downloadPct: 40 }))).toContain("40%");
    expect(detailsForRow(row({ stage: "whisper", whisperPct: 12 }))).toContain("12%");
    expect(detailsForRow(row({ stage: "queued" }))).toContain("Waiting");
  });
});
