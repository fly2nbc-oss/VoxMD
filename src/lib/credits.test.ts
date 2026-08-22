import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { allCredits, CREDITS } from "./credits";

describe("CREDITS", () => {
  it("names every entry once", () => {
    const names = allCredits().map((c) => c.name);
    expect(new Set(names).size, names.join(", ")).toBe(names.length);
  });

  /** A credit with no licence is not a credit. */
  it("states a licence and a reachable-looking URL for each", () => {
    for (const c of allCredits()) {
      expect(c.license.trim(), c.name).not.toBe("");
      expect(c.url, c.name).toMatch(/^https:\/\/[^\s]+$/);
    }
  });

  it("uses only keys the catalogue defines", () => {
    for (const group of CREDITS) {
      expect(Object.keys(en), group.title).toContain(group.title);
      for (const c of group.items) {
        if (c.note) expect(Object.keys(en), c.note).toContain(c.note);
      }
    }
  });

  /** Symphonia is the one copyleft dependency; losing that note loses the
   *  only hint that this project carries an MPL obligation. */
  it("keeps the copyleft dependency flagged", () => {
    const symphonia = allCredits().find((c) => c.name === "Symphonia");
    expect(symphonia?.license).toBe("MPL-2.0");
    expect(symphonia?.note).toBeTruthy();
  });

  it("credits the things the app downloads at runtime", () => {
    const names = allCredits().map((c) => c.name);
    expect(names).toContain("Whisper");
    expect(names).toContain("ONNX Runtime");
    expect(names).toContain("pyannote segmentation-3.0");
    expect(names).toContain("WeSpeaker CAM++");
  });
});
