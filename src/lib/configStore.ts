import { defaultConfig } from "../defaults";
import type { AppConfig, LlmProvider, PodcastRecent } from "../types";

export const STORE_FILE = "voxmd-settings.json";
export const CONFIG_KEY = "appConfig";
export const QUEUE_KEY = "queueItems";

export const PODCAST_RECENTS_MAX = 10;

const LLM_PROVIDERS: LlmProvider[] = ["deepseek", "openrouter", "custom"];

function asLlmProvider(raw: unknown, apiBaseUrl: string): LlmProvider {
  if (typeof raw === "string" && LLM_PROVIDERS.includes(raw as LlmProvider)) {
    return raw as LlmProvider;
  }
  const url = apiBaseUrl.trim().replace(/\/+$/, "").toLowerCase();
  if (url.includes("openrouter.ai")) return "openrouter";
  if (url.includes("deepseek.com") || url.length === 0) return "deepseek";
  return "custom";
}

function asBool(raw: unknown, fallback: boolean): boolean {
  return typeof raw === "boolean" ? raw : fallback;
}

function asMaxSpeakers(raw: unknown): number {
  if (typeof raw !== "number" || !Number.isFinite(raw)) return 0;
  return Math.max(0, Math.min(20, Math.round(raw)));
}

/** Explicit field picking also drops keys from older versions (temperature, maxTokens, …). */
export function mergeConfig(saved: Partial<AppConfig> | null | undefined): AppConfig {
  const base = defaultConfig();
  if (!saved) return base;
  const apiBaseUrl = saved.apiBaseUrl?.trim() ? saved.apiBaseUrl : base.apiBaseUrl;
  return {
    apiKey: saved.apiKey?.trim() ?? "",
    apiBaseUrl,
    apiModel: saved.apiModel?.trim() ? saved.apiModel : base.apiModel,
    llmProvider: asLlmProvider(saved.llmProvider, apiBaseUrl),
    whisperModel: saved.whisperModel?.trim() ? saved.whisperModel : base.whisperModel,
    language: saved.language?.trim() ? saved.language : base.language,
    summaryLanguage: saved.summaryLanguage?.trim()
      ? saved.summaryLanguage.trim()
      : base.summaryLanguage,
    useGpu: saved.useGpu ?? base.useGpu,
    deleteSourceAfterSuccess: saved.deleteSourceAfterSuccess ?? base.deleteSourceAfterSuccess,
    includeMeta: saved.includeMeta ?? base.includeMeta,
    includeSummary: saved.includeSummary ?? base.includeSummary,
    includeTranscript: saved.includeTranscript ?? base.includeTranscript,
    preventSleep: asBool(saved.preventSleep, base.preventSleep),
    diarizationEnabled: asBool(saved.diarizationEnabled, base.diarizationEnabled),
    maxSpeakers: asMaxSpeakers(saved.maxSpeakers),
    dictationModel: saved.dictationModel?.trim() ? saved.dictationModel : base.dictationModel,
    microphoneName: saved.microphoneName ?? base.microphoneName,
    podcastOutputDir: saved.podcastOutputDir ?? base.podcastOutputDir,
    podcastRecents: normalizePodcastRecents(saved.podcastRecents),
  };
}

/** Defensive: the store file is user-editable and may carry entries from older versions. */
export function normalizePodcastRecents(raw: unknown): PodcastRecent[] {
  if (!Array.isArray(raw)) return [];
  const out: PodcastRecent[] = [];
  const seen = new Set<string>();
  for (const entry of raw) {
    if (!entry || typeof entry !== "object") continue;
    const candidate = entry as Partial<PodcastRecent>;
    const feedUrl = typeof candidate.feedUrl === "string" ? candidate.feedUrl.trim() : "";
    const outputDir = typeof candidate.outputDir === "string" ? candidate.outputDir.trim() : "";
    if (!feedUrl || !outputDir || seen.has(feedUrl)) continue;
    seen.add(feedUrl);
    const rawTitle = typeof candidate.feedTitle === "string" ? candidate.feedTitle.trim() : "";
    out.push({ feedUrl, outputDir, feedTitle: rawTitle || undefined });
    if (out.length >= PODCAST_RECENTS_MAX) break;
  }
  return out;
}

export function rememberPodcastRecent(
  config: AppConfig,
  feedUrl: string,
  outputDir: string,
  feedTitle?: string,
): AppConfig {
  const url = feedUrl.trim();
  const dir = outputDir.trim();
  const title = feedTitle?.trim() || undefined;
  const rest = config.podcastRecents.filter((r) => r.feedUrl !== url);
  return {
    ...config,
    podcastOutputDir: dir,
    podcastRecents: [{ feedUrl: url, outputDir: dir, feedTitle: title }, ...rest].slice(
      0,
      PODCAST_RECENTS_MAX,
    ),
  };
}

export function isSummarySystemLanguage(lang: string): boolean {
  return lang.trim().toLowerCase() === "system";
}

export function isTranscriptionAuto(lang: string): boolean {
  const t = lang.trim().toLowerCase();
  return t === "auto" || t === "";
}
