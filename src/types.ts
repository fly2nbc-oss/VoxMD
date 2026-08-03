export interface AppConfig {
  apiKey: string;
  apiBaseUrl: string;
  apiModel: string;
  /** Model name ("turbo", "large-v3", …) or absolute path to local .bin/.gguf file */
  whisperModel: string;
  /** `"auto"` for Whisper language detection, or ISO 639-1 code (e.g. `de`). */
  language: string;
  /** `"system"` or ISO 639-1 code for LLM summary output */
  summaryLanguage: string;
  useGpu: boolean;
  /** After a successful export, delete the audio file only — never the Markdown. */
  deleteSourceAfterSuccess: boolean;
  /** Markdown output: metadata block (episode/file info) at the top. */
  includeMeta: boolean;
  /** Markdown output: LLM summary. Only effective when an API key is set; skipped otherwise. */
  includeSummary: boolean;
  /** Markdown output: raw Whisper transcript section. */
  includeTranscript: boolean;
  /** Last used output folder for podcast episode Markdown files. */
  podcastOutputDir: string;
  /** Recently used podcast feed URL + output directory pairs (UI only). */
  podcastRecents: PodcastRecent[];
}

/** One entry in the podcast-dialog recent list. */
export interface PodcastRecent {
  feedUrl: string;
  outputDir: string;
  /** Feed title from the last successful load, if known. */
  feedTitle?: string;
}

export interface EpisodeMeta {
  feedTitle: string;
  title: string;
  date?: string | null;
  link?: string | null;
  /** Target folder for the generated .md (episodes have no local source). */
  outputDir: string;
}

/** One entry in the processing queue: a local audio file or a podcast episode. */
export interface QueueItem {
  /** Stable key: local path or episode audio URL. */
  id: string;
  kind: "local" | "podcast";
  /** Local file path or episode audio URL. */
  source: string;
  displayName: string;
  episode?: EpisodeMeta | null;
}

/** Payload of the `job_progress` event (`pipeline.rs::JobProgressPayload`). */
export interface JobProgressPayload {
  /** Queue item id — the field is named `path` for historical reasons. */
  path: string;
  displayName: string;
  /** queued, download, whisper, llm, done, skipped, error */
  stage: string;
  whisperPct?: number;
  downloadPct?: number;
  overall?: { completed: number; total: number; pct: number };
  message?: string;
}

/** One row of the queue table: the latest progress payload for that item. */
export type JobRow = JobProgressPayload;

/** Payload of the `batch_complete` event. */
export interface BatchCompletePayload {
  total: number;
  cancelled?: boolean;
  error?: string;
}

/** Payload of the `model_download_progress` event. */
export interface ModelDownloadPayload {
  stage: string;
  model?: string;
  downloaded?: number;
  total?: number;
  pct?: number;
}

/** One entry in the whisper model dropdown (`list_whisper_models`). */
export interface WhisperModelInfo {
  name: string;
  sizeHint: string;
  cached: boolean;
}

/** A failed queue entry, collected for the error panel. */
export interface JobError {
  id: string;
  displayName: string;
  message: string;
}

/** Episode as returned by the `fetch_podcast_feed` command. */
export interface EpisodeInfo {
  feedTitle: string;
  title: string;
  date?: string | null;
  link?: string | null;
  audioUrl: string;
}
