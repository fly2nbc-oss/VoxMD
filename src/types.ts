export interface AppConfig {
  apiKey: string;
  apiBaseUrl: string;
  apiModel: string;
  temperature: number;
  maxTokens: number;
  transcriptChunkChars: number;
  /** Model name ("turbo", "large-v3", …) or absolute path to local .bin/.gguf file */
  whisperModel: string;
  whisperThreads: number | null;
  language: string;
  /** `"system"` or ISO 639-1 code for LLM summary output */
  summaryLanguage: string;
  useGpu: boolean;
  deleteSourceAfterSuccess: boolean;
}
