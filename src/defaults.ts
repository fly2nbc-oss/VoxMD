import type { AppConfig } from "./types";

/** Default settings; API key is entered by the user and stored locally only. */
export const defaultConfig = (): AppConfig => ({
  apiKey: "",
  apiBaseUrl: "https://api.deepseek.com",
  apiModel: "deepseek-v4-pro",
  temperature: 0.7,
  maxTokens: 65536,
  transcriptChunkChars: 32768,
  whisperModel: "turbo",
  whisperThreads: null,
  language: "de",
  summaryLanguage: "system",
  useGpu: true,
  deleteSourceAfterSuccess: true,
});
