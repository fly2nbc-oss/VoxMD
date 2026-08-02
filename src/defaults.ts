import type { AppConfig } from "./types";

/** Default settings; API key is entered by the user and stored locally only. */
export const defaultConfig = (): AppConfig => ({
  apiKey: "",
  apiBaseUrl: "https://api.deepseek.com",
  apiModel: "deepseek-v4-pro",
  whisperModel: "turbo",
  language: "auto",
  summaryLanguage: "system",
  useGpu: true,
  deleteSourceAfterSuccess: false,
  includeMeta: true,
  includeSummary: true,
  includeTranscript: true,
  podcastOutputDir: "",
  podcastRecents: [],
});
