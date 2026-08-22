import { describe, expect, it } from "vitest";
import {
  asMaxSpeakers,
  MAX_SPEAKERS,
  mergeConfig,
  normalizePodcastRecents,
  PODCAST_RECENTS_MAX,
  rememberPodcastRecent,
} from "./configStore";
import { defaultConfig } from "../defaults";

describe("mergeConfig", () => {
  it("fills empty strings from defaults and keeps valid overrides", () => {
    const merged = mergeConfig({
      apiKey: "  k  ",
      apiBaseUrl: "",
      apiModel: "  ",
      whisperModel: "small",
      useGpu: false,
    });
    expect(merged.apiKey).toBe("k");
    expect(merged.apiBaseUrl).toBe(defaultConfig().apiBaseUrl);
    expect(merged.apiModel).toBe(defaultConfig().apiModel);
    expect(merged.whisperModel).toBe("small");
    expect(merged.useGpu).toBe(false);
    expect(merged.llmProvider).toBe("deepseek");
    expect(merged.preventSleep).toBe(true);
    expect(merged.diarizationEnabled).toBe(false);
    expect(merged.dictationModel).toBe("small");
  });

  it("clamps maxSpeakers to the shared cap", () => {
    expect(mergeConfig({ maxSpeakers: 20 } as never).maxSpeakers).toBe(MAX_SPEAKERS);
    expect(mergeConfig({ maxSpeakers: -3 } as never).maxSpeakers).toBe(0);
    expect(mergeConfig({ maxSpeakers: 2 } as never).maxSpeakers).toBe(2);
  });

  it("infers OpenRouter from a stored URL when provider is missing", () => {
    const merged = mergeConfig({
      apiBaseUrl: "https://openrouter.ai/api/v1",
    } as never);
    expect(merged.llmProvider).toBe("openrouter");
  });

  it("returns defaults for null/undefined", () => {
    expect(mergeConfig(null)).toEqual(defaultConfig());
    expect(mergeConfig(undefined)).toEqual(defaultConfig());
  });
});

describe("asMaxSpeakers", () => {
  it("rounds, clamps and rejects non-numbers", () => {
    expect(asMaxSpeakers(3.4)).toBe(3);
    expect(asMaxSpeakers(99)).toBe(MAX_SPEAKERS);
    expect(asMaxSpeakers(-1)).toBe(0);
    expect(asMaxSpeakers(Number.NaN)).toBe(0);
    expect(asMaxSpeakers("2")).toBe(0);
  });
});

describe("normalizePodcastRecents", () => {
  it("drops garbage, dedupes by feedUrl, and caps at max", () => {
    expect(normalizePodcastRecents(null)).toEqual([]);
    expect(normalizePodcastRecents("nope")).toEqual([]);
    const many = Array.from({ length: PODCAST_RECENTS_MAX + 3 }, (_, i) => ({
      feedUrl: `https://feed/${i}`,
      outputDir: `/out/${i}`,
      feedTitle: `F${i}`,
    }));
    const out = normalizePodcastRecents([
      { feedUrl: " https://a ", outputDir: " /x " },
      { feedUrl: "https://a", outputDir: "/y" },
      { feedUrl: "", outputDir: "/z" },
      ...many,
      null,
      42,
    ]);
    expect(out[0]).toEqual({ feedUrl: "https://a", outputDir: "/x", feedTitle: undefined });
    expect(out).toHaveLength(PODCAST_RECENTS_MAX);
  });
});

describe("rememberPodcastRecent", () => {
  it("prepends, dedupes, and caps", () => {
    let cfg = defaultConfig();
    cfg = rememberPodcastRecent(cfg, "https://a", "/a", "A");
    cfg = rememberPodcastRecent(cfg, "https://b", "/b", "B");
    cfg = rememberPodcastRecent(cfg, "https://a", "/a2", "A2");
    expect(cfg.podcastRecents[0]).toEqual({
      feedUrl: "https://a",
      outputDir: "/a2",
      feedTitle: "A2",
    });
    expect(cfg.podcastRecents.map((r) => r.feedUrl)).toEqual(["https://a", "https://b"]);
    expect(cfg.podcastOutputDir).toBe("/a2");
  });
});
