import type { AppConfig, LlmProvider } from "../types";

export interface LlmProviderPreset {
  id: LlmProvider;
  label: string;
  baseUrl: string;
  defaultModel: string;
}

export const LLM_PROVIDER_PRESETS: LlmProviderPreset[] = [
  {
    id: "deepseek",
    label: "Deepseek",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-v4-pro",
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    baseUrl: "https://openrouter.ai/api/v1",
    defaultModel: "google/gemini-2.5-flash",
  },
  {
    id: "custom",
    label: "Custom",
    baseUrl: "",
    defaultModel: "",
  },
];

export function presetFor(id: LlmProvider): LlmProviderPreset {
  return LLM_PROVIDER_PRESETS.find((p) => p.id === id) ?? LLM_PROVIDER_PRESETS[0];
}

/** Switching to a named provider fills URL and model; `custom` keeps the current values. */
export function applyProvider(config: AppConfig, id: LlmProvider): AppConfig {
  if (id === "custom") {
    return { ...config, llmProvider: id };
  }
  const preset = presetFor(id);
  return {
    ...config,
    llmProvider: id,
    apiBaseUrl: preset.baseUrl,
    apiModel: preset.defaultModel || config.apiModel,
  };
}
