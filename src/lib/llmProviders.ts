import type { AppConfig, LlmProvider } from "../types";

export interface LlmProviderPreset {
  id: LlmProvider;
  label: string;
  /** Endpoint root; `/chat/completions` and `/models` hang off it. */
  baseUrl: string;
  defaultModel: string;
  /** Runs on this machine — no key needed, and the key field can stay empty. */
  local?: boolean;
}

/**
 * Providers exposing an OpenAI-compatible `/chat/completions` and `/models`.
 * Each base URL was probed: all answer `/models` with 400/401 for a bad key
 * rather than 404, so the route exists and the model picker can fill itself.
 *
 * Anthropic and Gemini are reachable through their own OpenAI compatibility
 * layers, not their native APIs — hence the unusual path suffixes.
 */
export const LLM_PROVIDER_PRESETS: LlmProviderPreset[] = [
  {
    id: "deepseek",
    label: "DeepSeek",
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
    id: "openai",
    label: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4.1-mini",
  },
  {
    id: "anthropic",
    label: "Anthropic",
    baseUrl: "https://api.anthropic.com/v1",
    defaultModel: "claude-sonnet-5",
  },
  {
    id: "google",
    label: "Google Gemini",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    defaultModel: "gemini-2.5-flash",
  },
  {
    id: "mistral",
    label: "Mistral",
    baseUrl: "https://api.mistral.ai/v1",
    defaultModel: "mistral-medium-latest",
  },
  {
    id: "groq",
    label: "Groq",
    baseUrl: "https://api.groq.com/openai/v1",
    defaultModel: "llama-3.3-70b-versatile",
  },
  {
    id: "xai",
    label: "xAI Grok",
    baseUrl: "https://api.x.ai/v1",
    defaultModel: "grok-4",
  },
  {
    id: "together",
    label: "Together AI",
    baseUrl: "https://api.together.xyz/v1",
    defaultModel: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
  },
  {
    id: "ollama",
    label: "Ollama (local)",
    baseUrl: "http://localhost:11434/v1",
    defaultModel: "llama3.1",
    local: true,
  },
  {
    id: "lmstudio",
    label: "LM Studio (local)",
    baseUrl: "http://localhost:1234/v1",
    defaultModel: "",
    local: true,
  },
  {
    id: "custom",
    label: "Custom",
    baseUrl: "",
    defaultModel: "",
  },
];

/**
 * Mirrors `is_local_endpoint` in `src-tauri/src/config.rs`; a Rust test asserts
 * the two agree. A model server on this machine authenticates nothing, so the
 * summary must not be treated as disabled just because the key field is empty.
 */
export function isLocalEndpoint(apiBaseUrl: string): boolean {
  const url = apiBaseUrl.trim().toLowerCase();
  const rest = url.replace(/^https?:\/\//, "");
  const authority = rest.split(/[/?#]/)[0] ?? "";
  const host = authority.replace(/:\d+$/, "").replace(/^\[|\]$/g, "");
  return (
    host === "localhost" ||
    host === "127.0.0.1" ||
    host === "0.0.0.0" ||
    host === "::1" ||
    host.endsWith(".localhost")
  );
}

/** True when a summary would actually run with this configuration. */
export function summaryWouldRun(config: {
  includeSummary: boolean;
  apiKey: string;
  apiBaseUrl: string;
}): boolean {
  return (
    config.includeSummary &&
    (config.apiKey.trim() !== "" || isLocalEndpoint(config.apiBaseUrl))
  );
}

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

/**
 * Best guess for a store written before `llmProvider` existed, or edited by
 * hand. Matched on host so a trailing path or `/v1` suffix does not matter.
 */
export function providerForUrl(apiBaseUrl: string): LlmProvider {
  const url = apiBaseUrl.trim().toLowerCase();
  if (url.length === 0) return "deepseek";
  const match = LLM_PROVIDER_PRESETS.find((p) => {
    if (!p.baseUrl) return false;
    const host = p.baseUrl.replace(/^https?:\/\//, "").split("/")[0];
    return url.includes(host);
  });
  return match?.id ?? "custom";
}
