import type { AppConfig } from "./types";

/** Defaults from API_KEY_Deepseek.env (Audio-Transcript); API key is not imported. */
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
  useGpu: true,
  deleteSourceAfterSuccess: true,
  whisperVerbose: false,
});
