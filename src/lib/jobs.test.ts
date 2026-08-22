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
    expect(badgeForStage("done").label).toBe("Done");
    expect(badgeForStage("error").className).toBe("badge-error");
    expect(badgeForStage("whisper").label).toBe("Whisper");
    expect(badgeForStage("diarize").label).toBe("Speakers");
    expect(badgeForStage("diarize").className).toBe("badge-active");
  });
});

describe("detailsForRow", () => {
  it("includes download and whisper percentages", () => {
    expect(detailsForRow(row({ stage: "download", downloadPct: 40 }))).toContain("40%");
    expect(detailsForRow(row({ stage: "whisper", whisperPct: 12 }))).toContain("12%");
    expect(detailsForRow(row({ stage: "queued" }))).toContain("Waiting");
    expect(detailsForRow(row({ stage: "diarize" }))).toContain("Separating");
  });
});
