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
  useGpu: boolean;
  deleteSourceAfterSuccess: boolean;
  whisperVerbose: boolean;
}
