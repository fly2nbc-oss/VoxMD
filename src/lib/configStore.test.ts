import { describe, expect, it } from "vitest";
import {
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
  });

  it("returns defaults for null/undefined", () => {
    expect(mergeConfig(null)).toEqual(defaultConfig());
    expect(mergeConfig(undefined)).toEqual(defaultConfig());
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
