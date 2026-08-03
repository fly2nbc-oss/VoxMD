import { defaultConfig } from "../defaults";
import type { AppConfig, PodcastRecent } from "../types";

export const STORE_FILE = "voxmd-settings.json";
export const CONFIG_KEY = "appConfig";

export const PODCAST_RECENTS_MAX = 10;

/** Explicit field picking also drops keys from older versions (temperature, maxTokens, …). */
export function mergeConfig(saved: Partial<AppConfig> | null | undefined): AppConfig {
  const base = defaultConfig();
  if (!saved) return base;
  return {
    apiKey: saved.apiKey?.trim() ?? "",
    apiBaseUrl: saved.apiBaseUrl?.trim() ? saved.apiBaseUrl : base.apiBaseUrl,
    apiModel: saved.apiModel?.trim() ? saved.apiModel : base.apiModel,
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
