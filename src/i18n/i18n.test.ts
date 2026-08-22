import { describe, expect, it } from "vitest";
import { de } from "./de";
import { en } from "./en";
import { es } from "./es";
import { fr } from "./fr";
import { it as itMessages } from "./it";
import {
  asUiLanguageSetting,
  detectUiLanguage,
  format,
  isUiLanguage,
  messagesFor,
  resolveUiLanguage,
  UI_LANGUAGES,
  UI_LANGUAGE_NAMES,
} from "./index";

const LOCALES = { en, de, fr, it: itMessages, es } as const;

describe("catalogues", () => {
  it("cover exactly the same keys as English", () => {
    const expected = Object.keys(en).sort();
    for (const [name, messages] of Object.entries(LOCALES)) {
      expect(Object.keys(messages).sort(), name).toEqual(expected);
    }
  });

  it("leave no entry blank", () => {
    for (const [name, messages] of Object.entries(LOCALES)) {
      for (const [key, value] of Object.entries(messages)) {
        expect(value.trim(), `${name}/${key}`).not.toBe("");
      }
    }
  });

  /** A dropped or renamed placeholder shows up as a literal `{count}` at runtime. */
  it("keep the same placeholders as English", () => {
    const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const [name, messages] of Object.entries(LOCALES)) {
      for (const key of Object.keys(en) as Array<keyof typeof en>) {
        expect(placeholders(messages[key]), `${name}/${key}`).toEqual(placeholders(en[key]));
      }
    }
  });

  it("has a name for every shipped locale", () => {
    for (const code of UI_LANGUAGES) {
      expect(UI_LANGUAGE_NAMES[code]).toBeTruthy();
      expect(messagesFor(code)).toBe(LOCALES[code]);
    }
  });
});

describe("format", () => {
  it("substitutes named placeholders", () => {
    expect(format(en, "status.overall", { done: 2, total: 5 })).toBe("Overall: 2 / 5 done (MD)");
  });

  it("leaves an unmatched placeholder visible instead of blanking it", () => {
    expect(format(en, "status.overall", { done: 2 })).toContain("{total}");
  });

  it("returns the template untouched when no params are given", () => {
    expect(format(de, "common.save")).toBe("Speichern");
  });
});

describe("language resolution", () => {
  it("accepts only shipped codes", () => {
    expect(isUiLanguage("de")).toBe(true);
    expect(isUiLanguage("pt")).toBe(false);
    expect(asUiLanguageSetting("fr")).toBe("fr");
    expect(asUiLanguageSetting("system")).toBe("system");
    expect(asUiLanguageSetting("klingon")).toBe("system");
    expect(asUiLanguageSetting(undefined)).toBe("system");
  });

  it("matches the primary subtag and falls back to English", () => {
    expect(detectUiLanguage(["de-AT", "en"])).toBe("de");
    expect(detectUiLanguage(["pt-BR", "it-CH"])).toBe("it");
    expect(detectUiLanguage(["ja"])).toBe("en");
    expect(detectUiLanguage([])).toBe("en");
  });

  it("passes explicit settings through", () => {
    expect(resolveUiLanguage("es")).toBe("es");
  });
});
