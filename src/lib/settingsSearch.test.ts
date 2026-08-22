import { describe, expect, it } from "vitest";
import { defaultConfig } from "../defaults";
import { en } from "../i18n/en";
import { format, messagesFor, type MessageKey } from "../i18n";
import {
  changedFields,
  changedSections,
  searchSettings,
  SETTINGS_INDEX,
  SETTINGS_SECTIONS,
} from "./settingsSearch";
import type { AppConfig } from "../types";

const t = (key: MessageKey) => format(en, key);
const tDe = (key: MessageKey) => format(messagesFor("de"), key);

describe("SETTINGS_INDEX", () => {
  it("has unique ids and only known sections", () => {
    const ids = SETTINGS_INDEX.map((e) => e.id);
    expect(new Set(ids).size, ids.join(", ")).toBe(ids.length);
    for (const e of SETTINGS_INDEX) {
      expect(SETTINGS_SECTIONS, e.id).toContain(e.section);
    }
  });

  it("points every field at a real AppConfig key", () => {
    const keys = new Set(Object.keys(defaultConfig()));
    for (const e of SETTINGS_INDEX) {
      if (e.field) expect(keys, e.id).toContain(e.field);
    }
  });

  /** A row nobody can reach through search is a row nobody can find. */
  it("covers every section", () => {
    for (const section of SETTINGS_SECTIONS) {
      expect(SETTINGS_INDEX.some((e) => e.section === section), section).toBe(true);
    }
  });
});

describe("searchSettings", () => {
  it("is empty for a blank query rather than listing everything", () => {
    expect(searchSettings("", t)).toEqual([]);
    expect(searchSettings("   ", t)).toEqual([]);
  });

  it("matches the translated label in the active language", () => {
    expect(searchSettings("Sprecher", tDe).map((e) => e.id)).toContain("diarizationEnabled");
    expect(searchSettings("speaker", t).map((e) => e.id)).toContain("diarizationEnabled");
  });

  it("matches technical keywords the labels never show", () => {
    // "Vulkan" appears in no visible label — only as a keyword.
    expect(searchSettings("vulkan", t).map((e) => e.id)).toEqual(["useGpu"]);
    expect(searchSettings("ollama", t).map((e) => e.id)).toEqual(["llmProvider"]);
  });

  it("folds diacritics so an ASCII query still finds an umlaut label", () => {
    expect(searchSettings("schlussel", tDe).map((e) => e.id)).toContain("apiKey");
    expect(searchSettings("oberflache", tDe).map((e) => e.id)).toContain("uiLanguage");
  });

  it("requires every term, so more words narrow the result", () => {
    const one = searchSettings("sprache", tDe);
    const two = searchSettings("sprache zusammenfassung", tDe);
    expect(one.length).toBeGreaterThan(two.length);
    expect(two.map((e) => e.id)).toEqual(["summaryLanguage"]);
  });

  it("returns nothing for a query that matches nothing", () => {
    expect(searchSettings("zzzz", t)).toEqual([]);
  });
});

describe("changedFields / changedSections", () => {
  const base = defaultConfig();

  it("reports nothing when config equals what is on disk", () => {
    expect(changedFields(base, base)).toEqual([]);
    expect(changedSections(base, base).size).toBe(0);
  });

  it("names the changed keys and their sections", () => {
    const edited: AppConfig = { ...base, useGpu: !base.useGpu, apiModel: "gpt-4.1" };
    expect(changedFields(edited, base).sort()).toEqual(["apiModel", "useGpu"]);
    expect([...changedSections(edited, base)].sort()).toEqual(["llm", "whisper"]);
  });

  /** Recent feeds are history and persist immediately — never an unsaved edit. */
  it("ignores podcastRecents", () => {
    const edited: AppConfig = {
      ...base,
      podcastRecents: [{ feedUrl: "https://a", outputDir: "/x" }],
    };
    expect(changedFields(edited, base)).toEqual([]);
  });

  it("leaves a changed field out of the dots when no row indexes it", () => {
    // `includeMeta` lives in the toolbar, not the drawer.
    const edited: AppConfig = { ...base, includeMeta: !base.includeMeta };
    expect(changedFields(edited, base)).toEqual(["includeMeta"]);
    expect(changedSections(edited, base).size).toBe(0);
  });
});
