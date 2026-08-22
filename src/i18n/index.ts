import { de } from "./de";
import { en, type MessageKey, type Messages } from "./en";
import { es } from "./es";
import { fr } from "./fr";
import { it } from "./it";

export type { MessageKey, Messages };

/** Locales the UI ships. `system` follows the OS/browser locale. */
export const UI_LANGUAGES = ["en", "de", "fr", "it", "es"] as const;
export type UiLanguage = (typeof UI_LANGUAGES)[number];
/** Value stored in settings: a concrete locale, or `system`. */
export type UiLanguageSetting = UiLanguage | "system";

/** Endonyms — a language list is easier to scan in each language's own name. */
export const UI_LANGUAGE_NAMES: Record<UiLanguage, string> = {
  en: "English",
  de: "Deutsch",
  fr: "Français",
  it: "Italiano",
  es: "Español",
};

const CATALOGUES: Record<UiLanguage, Messages> = { en, de, fr, it, es };

export function isUiLanguage(value: unknown): value is UiLanguage {
  return typeof value === "string" && (UI_LANGUAGES as readonly string[]).includes(value);
}

export function asUiLanguageSetting(value: unknown): UiLanguageSetting {
  if (value === "system" || isUiLanguage(value)) return value;
  return "system";
}

/** First OS/browser preference the UI ships, else English. */
export function detectUiLanguage(
  preferred: readonly string[] = typeof navigator === "undefined" ? [] : navigator.languages,
): UiLanguage {
  for (const tag of preferred) {
    const primary = tag.split(/[-_]/)[0]?.toLowerCase();
    if (isUiLanguage(primary)) return primary;
  }
  return "en";
}

export function resolveUiLanguage(setting: UiLanguageSetting): UiLanguage {
  return setting === "system" ? detectUiLanguage() : setting;
}

export function messagesFor(lang: UiLanguage): Messages {
  return CATALOGUES[lang];
}

/**
 * Looks up `key` and substitutes `{name}` placeholders.
 *
 * A placeholder with no matching parameter is left in place rather than blanked:
 * `Overall: {done}` is a visible bug report, an empty gap is not.
 */
export function format(
  messages: Messages,
  key: MessageKey,
  params?: Record<string, string | number>,
): string {
  const template = messages[key] ?? en[key] ?? key;
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}
