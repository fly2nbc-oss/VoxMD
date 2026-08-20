import { describe, expect, it } from "vitest";
import { defaultConfig } from "../defaults";
import { applyProvider } from "./llmProviders";

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
});
