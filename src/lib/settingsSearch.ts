import type { MessageKey } from "../i18n";
import type { AppConfig } from "../types";

/** Rail order, top to bottom. `about` is not a setting and sits apart. */
export const SETTINGS_SECTIONS = ["appearance", "whisper", "llm", "dictation"] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];
export type SettingsTab = SettingsSection | "about";

/**
 * One searchable setting.
 *
 * `field` is the `AppConfig` key the row edits — it drives both the unsaved-dot
 * per section and the highlight after a search jump, so a row without a stored
 * field (a button, a read-only line) simply omits it.
 */
export interface SettingIndexEntry {
  id: string;
  section: SettingsSection;
  field?: keyof AppConfig;
  label: MessageKey;
  hint: MessageKey;
  /** Extra words to match on, beyond the translated label and hint. */
  keywords: string;
}

export const SETTINGS_INDEX: SettingIndexEntry[] = [
  {
    id: "theme",
    section: "appearance",
    label: "settings.theme",
    hint: "search.themeHint",
    keywords: "theme thema hell dunkel dark light erscheinungsbild aussehen apariencia apparence tema",
  },
  {
    id: "uiLanguage",
    section: "appearance",
    field: "uiLanguage",
    label: "settings.uiLanguage",
    hint: "search.uiLanguageHint",
    keywords: "sprache language oberflaeche oberfläche interface ui idioma langue lingua",
  },
  {
    id: "whisperModel",
    section: "whisper",
    field: "whisperModel",
    label: "settings.whisperModel",
    hint: "search.whisperModelHint",
    keywords: "whisper modell model genauigkeit qualitaet qualität turbo large tiny small geschwindigkeit accuracy",
  },
  {
    id: "language",
    section: "whisper",
    field: "language",
    label: "settings.transcriptionLanguage",
    hint: "search.transcriptionLanguageHint",
    keywords: "sprache language audio gesprochen spoken iso erkennen detect idioma langue lingua",
  },
  {
    id: "diarizationEnabled",
    section: "whisper",
    field: "diarizationEnabled",
    label: "settings.speakerLabels",
    hint: "search.speakersHint",
    keywords: "sprecher speaker diarisierung diarization interview personen locuteur parlante hablante",
  },
  {
    id: "maxSpeakers",
    section: "whisper",
    field: "maxSpeakers",
    label: "search.speakerCount",
    hint: "search.speakerCountHint",
    keywords: "sprecher anzahl personen zwei interview automatisch count number",
  },
  {
    id: "useGpu",
    section: "whisper",
    field: "useGpu",
    label: "settings.useGpu",
    hint: "search.gpuHint",
    keywords: "gpu grafikkarte graphics vulkan hardware beschleunigung acceleration radeon nvidia",
  },
  {
    id: "preventSleep",
    section: "whisper",
    field: "preventSleep",
    label: "settings.preventSleep",
    hint: "search.preventSleepHint",
    keywords: "ruhezustand schlaf standby energie wach sleep suspend veille sospensione",
  },
  {
    id: "whisperCache",
    section: "whisper",
    label: "search.modelCache",
    hint: "search.modelCacheHint",
    keywords: "cache speicher platte festplatte modelle leeren loeschen löschen disk storage",
  },
  {
    id: "llmProvider",
    section: "llm",
    field: "llmProvider",
    label: "settings.provider",
    hint: "search.providerHint",
    keywords: "anbieter provider dienst llm openai anthropic deepseek ollama gemini mistral groq openrouter lmstudio",
  },
  {
    id: "apiKey",
    section: "llm",
    field: "apiKey",
    label: "settings.apiKey",
    hint: "search.apiKeyHint",
    keywords: "api key schluessel schlüssel token passwort password clave clef chiave",
  },
  {
    id: "apiModel",
    section: "llm",
    field: "apiModel",
    label: "settings.model",
    hint: "search.modelHint",
    keywords: "modell model gpt claude gemini llama",
  },
  {
    id: "apiBaseUrl",
    section: "llm",
    field: "apiBaseUrl",
    label: "settings.baseUrl",
    hint: "search.baseUrlHint",
    keywords: "url endpoint adresse host basis base server",
  },
  {
    id: "summaryLanguage",
    section: "llm",
    field: "summaryLanguage",
    label: "settings.summaryLanguage",
    hint: "search.summaryLanguageHint",
    keywords: "sprache language zusammenfassung summary idioma langue lingua resumen",
  },
  {
    id: "dictationModel",
    section: "dictation",
    field: "dictationModel",
    label: "settings.dictationModel",
    hint: "search.dictationModelHint",
    keywords: "diktat dictation diktieren mikrofon microphone live modell model",
  },
];

/** Diacritics folded so "oberflache" finds "Oberfläche". */
function fold(value: string): string {
  return value
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "");
}

/**
 * Matches every whitespace-separated term against the translated label, the
 * translated hint and the keyword list — so a query works in the active UI
 * language and in the untranslated technical terms alike.
 */
export function searchSettings(
  query: string,
  t: (key: MessageKey) => string,
  index: SettingIndexEntry[] = SETTINGS_INDEX,
): SettingIndexEntry[] {
  const terms = fold(query).split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [];
  return index.filter((entry) => {
    const haystack = fold(`${t(entry.label)} ${t(entry.hint)} ${entry.keywords}`);
    return terms.every((term) => haystack.includes(term));
  });
}

/** `AppConfig` keys that differ between the live edits and what is on disk. */
export function changedFields(config: AppConfig, saved: AppConfig): Array<keyof AppConfig> {
  return (Object.keys(config) as Array<keyof AppConfig>).filter((key) => {
    const a = config[key];
    const b = saved[key];
    // `podcastRecents` is history, not a setting, and is persisted immediately.
    if (key === "podcastRecents") return false;
    return a !== b;
  });
}

/** Sections carrying at least one unsaved change, for the rail's dot. */
export function changedSections(
  config: AppConfig,
  saved: AppConfig,
  index: SettingIndexEntry[] = SETTINGS_INDEX,
): Set<SettingsSection> {
  const fields = new Set(changedFields(config, saved).map(String));
  const out = new Set<SettingsSection>();
  for (const entry of index) {
    if (entry.field && fields.has(String(entry.field))) out.add(entry.section);
  }
  return out;
}
