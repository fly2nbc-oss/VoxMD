import { describe, expect, it } from "vitest";
import { itemsToPersist, parseSavedQueue } from "./queuePersist";
import type { JobRow, QueueItem } from "../types";

const local: QueueItem = {
  id: "/a.mp3",
  kind: "local",
  source: "/a.mp3",
  displayName: "a.mp3",
};

const podcast: QueueItem = {
  id: "https://ex/a.mp3",
  kind: "podcast",
  source: "https://ex/a.mp3",
  displayName: "Ep",
  episode: { feedTitle: "Feed", title: "Ep", outputDir: "/out" },
};

function jobs(stage: string, outputPath?: string): Record<string, JobRow> {
  return {
    [local.id]: { path: local.id, displayName: local.displayName, stage, outputPath },
  };
}

describe("itemsToPersist", () => {
  it("drops completed exports and keeps everything else", () => {
    expect(itemsToPersist([local], jobs("done"))).toEqual([]);
    expect(itemsToPersist([local], jobs("error"))).toEqual([local]);
    expect(itemsToPersist([local], jobs("queued"))).toEqual([local]);
    expect(itemsToPersist([local], {})).toEqual([local]);
  });

  it("tells an already-exported skip apart from a cancelled one", () => {
    // Markdown exists: finished, nothing to restore on the next start.
    expect(itemsToPersist([local], jobs("skipped", "/out/Talk.md"))).toEqual([]);
    // Cancelled before it ran: still outstanding work.
    expect(itemsToPersist([local], jobs("skipped"))).toEqual([local]);
  });
});

describe("parseSavedQueue", () => {
  it("keeps well-formed local and podcast items", () => {
    expect(parseSavedQueue([local, podcast])).toEqual([local, podcast]);
  });

  it("drops garbage, duplicates, and podcasts without an output folder", () => {
    expect(parseSavedQueue(null)).toEqual([]);
    expect(parseSavedQueue("nope")).toEqual([]);
    expect(
      parseSavedQueue([
        local,
        local,
        { id: "x", kind: "podcast", source: "u", displayName: "n" },
        { id: "", kind: "local", source: "/z", displayName: "z" },
        42,
      ]),
    ).toEqual([local]);
  });
});
