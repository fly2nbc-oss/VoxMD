import { describe, expect, it } from "vitest";
import { defaultConfig } from "../defaults";
import {
  applyProvider,
  isLocalEndpoint,
  LLM_PROVIDER_PRESETS,
  presetFor,
  providerForUrl,
  summaryWouldRun,
} from "./llmProviders";

describe("applyProvider", () => {
  it("fills Deepseek and OpenRouter URLs and keeps custom values", () => {
    const base = defaultConfig();
    const openrouter = applyProvider(base, "openrouter");
    expect(openrouter.llmProvider).toBe("openrouter");
    expect(openrouter.apiBaseUrl).toBe("https://openrouter.ai/api/v1");
    expect(openrouter.apiModel).toBe("google/gemini-2.5-flash");

    const custom = applyProvider(openrouter, "custom");
    expect(custom.llmProvider).toBe("custom");
    expect(custom.apiBaseUrl).toBe(openrouter.apiBaseUrl);
    expect(custom.apiModel).toBe(openrouter.apiModel);

    const deepseek = applyProvider(custom, "deepseek");
    expect(deepseek.apiBaseUrl).toBe("https://api.deepseek.com");
    expect(deepseek.apiModel).toBe("deepseek-v4-pro");
  });

  it("fills every named provider with its own endpoint", () => {
    const base = defaultConfig();
    for (const preset of LLM_PROVIDER_PRESETS) {
      if (preset.id === "custom") continue;
      const next = applyProvider(base, preset.id);
      expect(next.llmProvider, preset.id).toBe(preset.id);
      expect(next.apiBaseUrl, preset.id).toBe(preset.baseUrl);
    }
  });
});

describe("LLM_PROVIDER_PRESETS", () => {
  it("has unique ids and one usable endpoint each", () => {
    const ids = LLM_PROVIDER_PRESETS.map((p) => p.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).toContain("custom");
    for (const p of LLM_PROVIDER_PRESETS) {
      expect(p.label.trim(), p.id).not.toBe("");
      if (p.id === "custom") continue;
      // The backend appends /chat/completions and /models, so a trailing slash
      // would produce a double slash.
      expect(p.baseUrl, p.id).toMatch(/^https?:\/\/[^\s]+[^/]$/);
    }
  });

  it("marks only the localhost providers as local", () => {
    for (const p of LLM_PROVIDER_PRESETS) {
      expect(p.local === true, p.id).toBe(p.baseUrl.includes("localhost"));
    }
  });

  it("falls back to the first preset for an unknown id", () => {
    expect(presetFor("nope" as never)).toBe(LLM_PROVIDER_PRESETS[0]);
  });
});

describe("providerForUrl", () => {
  it("recognises every preset host", () => {
    for (const p of LLM_PROVIDER_PRESETS) {
      if (p.id === "custom") continue;
      expect(providerForUrl(p.baseUrl), p.id).toBe(p.id);
    }
  });

  it("handles an empty, foreign or differently-suffixed URL", () => {
    expect(providerForUrl("")).toBe("deepseek");
    expect(providerForUrl("https://api.openai.com/v1/")).toBe("openai");
    expect(providerForUrl("https://llm.example.internal/v1")).toBe("custom");
  });
});

/** Kept identical to `config.rs::is_local_endpoint`; a Rust test reads these. */
export const LOCAL_ENDPOINT_CASES: Array<[string, boolean]> = [
  ["http://localhost:11434/v1", true],
  ["http://127.0.0.1:1234/v1", true],
  ["http://[::1]:8080/v1", true],
  ["https://LOCALHOST/v1", true],
  ["http://ollama.localhost/v1", true],
  ["https://api.openai.com/v1", false],
  ["https://api.deepseek.com", false],
  ["https://localhost.example.com/v1", false],
  ["https://evil.com/localhost", false],
  ["", false],
];

describe("isLocalEndpoint", () => {
  it("matches only loopback authorities", () => {
    for (const [url, expected] of LOCAL_ENDPOINT_CASES) {
      expect(isLocalEndpoint(url), url).toBe(expected);
    }
  });
});

describe("summaryWouldRun", () => {
  const cfg = (over: Partial<Parameters<typeof summaryWouldRun>[0]>) => ({
    includeSummary: true,
    apiKey: "",
    apiBaseUrl: "https://api.deepseek.com",
    ...over,
  });

  it("needs a key for a remote endpoint", () => {
    expect(summaryWouldRun(cfg({}))).toBe(false);
    expect(summaryWouldRun(cfg({ apiKey: "sk-x" }))).toBe(true);
  });

  it("needs no key for a model server on this machine", () => {
    expect(summaryWouldRun(cfg({ apiBaseUrl: "http://localhost:11434/v1" }))).toBe(true);
  });

  it("is off whenever the toolbar toggle is off", () => {
    expect(
      summaryWouldRun(cfg({ includeSummary: false, apiBaseUrl: "http://localhost:11434/v1" })),
    ).toBe(false);
  });
});
