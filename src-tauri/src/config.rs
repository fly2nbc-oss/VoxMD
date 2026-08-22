use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub api_key: String,
    pub api_base_url: String,
    pub api_model: String,
    /// Which preset filled `api_base_url` (`deepseek`, `openai`, `ollama`, …).
    /// Frontend-only bookkeeping — the backend always talks to `api_base_url`.
    /// Unknown or missing values mean Deepseek.
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    /// Whisper model: name ("turbo", "large-v3", "medium", "small", "base", "tiny")
    /// or absolute path to a local .bin / .gguf file.
    /// Aliases: whisperModelPath (old store key) → whisperModel.
    #[serde(alias = "whisperModelPath")]
    pub whisper_model: String,
    /// Transcription language: `"auto"` for Whisper detection, or ISO 639-1 (e.g. `de`).
    #[serde(default = "default_language")]
    pub language: String,
    /// Summary language: `"system"` or ISO 639-1 code (e.g. `de`, `en`).
    #[serde(default = "default_summary_language")]
    pub summary_language: String,
    /// Use GPU when the binary was built with `gpu-vulkan` and the Vulkan loader is present at runtime.
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    /// After a successful export, delete the audio file only (never the Markdown).
    #[serde(default)]
    pub delete_source_after_success: bool,
    /// Write a metadata block (episode/file info) at the top of the Markdown output.
    #[serde(default = "default_true")]
    pub include_meta: bool,
    /// Generate an LLM summary. Only effective with an API key (see `summary_enabled`).
    #[serde(default = "default_true")]
    pub include_summary: bool,
    /// Write the raw Whisper transcript into the Markdown output.
    #[serde(default = "default_true")]
    pub include_transcript: bool,
    /// Keep the machine from idle-sleeping while a batch runs.
    #[serde(default = "default_true")]
    pub prevent_sleep: bool,
    /// Run pyannote speaker diarization after Whisper and label transcript lines.
    #[serde(default)]
    pub diarization_enabled: bool,
    /// 0 = automatic speaker count; 1..=[`MAX_SPEAKERS_CAP`] is an exact count.
    #[serde(default)]
    pub max_speakers: u8,
    /// Whisper preset or path used only for live dictation.
    #[serde(default = "default_dictation_model")]
    pub dictation_model: String,
    /// Empty = system default input device.
    #[serde(default)]
    pub microphone_name: String,
    /// UI locale (`system`, `en`, `de`, `fr`, `it`, `es`). Frontend-only; the
    /// backend's own messages are English.
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    /// Last used output folder for podcast episode Markdown files (frontend convenience).
    #[serde(default)]
    pub podcast_output_dir: String,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "auto".to_string()
}

fn default_summary_language() -> String {
    "system".to_string()
}

fn default_llm_provider() -> String {
    "deepseek".to_string()
}

fn default_dictation_model() -> String {
    "small".to_string()
}

fn default_ui_language() -> String {
    "system".to_string()
}

/// Hard cap for speaker clustering (auto and explicit).
pub const MAX_SPEAKERS_CAP: u8 = 8;

/// Loopback hosts, matched on the authority so a path or port cannot fake one.
/// Mirrored by `isLocalEndpoint` in `src/lib/llmProviders.ts`.
pub fn is_local_endpoint(api_base_url: &str) -> bool {
    let url = api_base_url.trim().to_ascii_lowercase();
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(&url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = match authority.rsplit_once(':') {
        // `[::1]:1234` — keep the bracketed host, drop the port.
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => authority,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1") || host.ends_with(".localhost")
}

/// Normalize locale string to ISO 639-1 (two lowercase letters).
fn normalize_iso639_1(locale: &str) -> Option<String> {
    let primary = locale.split(&['-', '_', '.'][..]).next()?.trim();
    if primary.len() >= 2 && primary.is_ascii() {
        let code: String = primary.chars().take(2).collect();
        if code.chars().all(|c| c.is_ascii_alphabetic()) {
            return Some(code.to_lowercase());
        }
    }
    None
}

/// Resolve summary language setting to an ISO 639-1 code for LLM prompts.
pub fn resolve_summary_language(code: &str) -> String {
    let trimmed = code.trim();
    if trimmed.eq_ignore_ascii_case("system") {
        if let Some(locale) = sys_locale::get_locale() {
            if let Some(iso) = normalize_iso639_1(&locale) {
                return iso;
            }
        }
        return "en".to_string();
    }
    if trimmed.is_empty() {
        return "en".to_string();
    }
    normalize_iso639_1(trimmed).unwrap_or_else(|| trimmed.to_lowercase())
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_base_url: "https://api.deepseek.com".to_string(),
            api_model: "deepseek-v4-pro".to_string(),
            llm_provider: default_llm_provider(),
            whisper_model: "turbo".to_string(),
            language: default_language(),
            summary_language: default_summary_language(),
            use_gpu: default_true(),
            delete_source_after_success: false,
            include_meta: true,
            include_summary: true,
            include_transcript: true,
            prevent_sleep: true,
            diarization_enabled: false,
            max_speakers: 0,
            dictation_model: default_dictation_model(),
            microphone_name: String::new(),
            ui_language: default_ui_language(),
            podcast_output_dir: String::new(),
        }
    }
}

/// True when `whisper_model` looks like a filesystem path (not a preset name).
fn looks_like_whisper_path(model: &str) -> bool {
    let m = model.trim();
    m.starts_with('/')
        || m.starts_with('\\')
        || m.starts_with('.')
        || m.contains('/')
        || m.contains('\\')
        || (m.len() >= 2 && m.as_bytes()[1] == b':')
}

impl AppConfig {
    /// The summary is only generated when enabled AND an API key is present;
    /// without a key it is skipped silently instead of failing the run.
    pub fn summary_enabled(&self) -> bool {
        self.include_summary && (!self.api_key.trim().is_empty() || self.endpoint_is_local())
    }

    /// A model server on this machine (Ollama, LM Studio, llama.cpp …) needs no
    /// key. Without this the summary would be skipped in silence for exactly the
    /// providers where leaving the key blank is the normal setup.
    pub fn endpoint_is_local(&self) -> bool {
        is_local_endpoint(&self.api_base_url)
    }

    /// 0 = automatic; otherwise the exact speaker count, clamped to [`MAX_SPEAKERS_CAP`].
    pub fn speaker_count(&self) -> u8 {
        self.max_speakers.min(MAX_SPEAKERS_CAP)
    }

    pub fn validate_for_run(&self) -> Result<(), String> {
        if !self.summary_enabled() && !self.include_transcript {
            return Err(if self.include_summary {
                "No API key: the summary is skipped and the transcript is disabled — the output \
                 would be empty. Enter an API key or enable the transcript (toolbar)."
                    .to_string()
            } else {
                "Markdown output is empty: enable at least Summary or Transcript (toolbar)."
                    .to_string()
            });
        }
        // API access is only required when the summary will actually run.
        if self.summary_enabled() {
            let url = self.api_base_url.trim();
            if url.is_empty() {
                return Err("API base URL missing.".to_string());
            }
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err("API base URL must start with http:// or https://.".to_string());
            }
            if self.api_model.trim().is_empty() {
                return Err("API model missing.".to_string());
            }
        }
        let model = self.whisper_model.trim();
        if model.is_empty() {
            return Err(
                "Whisper model missing. Enter a name (turbo, large-v3, …) or a local file path."
                    .to_string(),
            );
        }
        if looks_like_whisper_path(model) {
            let p = std::path::Path::new(model);
            if !p.is_file() {
                return Err(format!("Whisper model file not found: {model}"));
            }
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext != "bin" && ext != "gguf" {
                return Err(format!(
                    "Whisper model must be a .bin or .gguf file, got: .{ext}"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_local_endpoint, normalize_iso639_1, resolve_summary_language, AppConfig,
        MAX_SPEAKERS_CAP,
    };

    #[test]
    fn resolve_explicit_iso_code() {
        assert_eq!(resolve_summary_language("en"), "en");
        assert_eq!(resolve_summary_language("  DE  "), "de");
    }

    #[test]
    fn resolve_empty_falls_back_to_en() {
        assert_eq!(resolve_summary_language(""), "en");
        assert_eq!(resolve_summary_language("   "), "en");
    }

    #[test]
    fn validate_rejects_bad_api_url() {
        let cfg = AppConfig {
            api_key: "k".into(),
            api_base_url: "ftp://example.com".into(),
            ..AppConfig::default()
        };
        assert!(cfg.validate_for_run().is_err());
    }

    #[test]
    fn validate_custom_whisper_path() {
        let cfg = AppConfig {
            api_key: "k".into(),
            whisper_model: "/no/such/model.gguf".into(),
            ..AppConfig::default()
        };
        let err = cfg.validate_for_run().unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn validate_skips_api_checks_without_summary() {
        // No API key set — still valid, since only the transcript is written.
        let cfg = AppConfig {
            include_summary: false,
            ..AppConfig::default()
        };
        assert!(cfg.validate_for_run().is_ok());
    }

    #[test]
    fn summary_without_key_is_skipped_not_an_error() {
        // Summary enabled but no key: run is valid, summary just won't happen.
        let cfg = AppConfig::default();
        assert!(cfg.include_summary);
        assert!(!cfg.summary_enabled());
        assert!(cfg.validate_for_run().is_ok());
    }

    #[test]
    fn defaults_cover_the_new_settings() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.llm_provider, "deepseek");
        assert!(cfg.prevent_sleep);
        assert!(!cfg.diarization_enabled);
        assert_eq!(cfg.max_speakers, 0);
        assert_eq!(cfg.speaker_count(), 0);
        let capped = AppConfig {
            max_speakers: 20,
            ..AppConfig::default()
        };
        assert_eq!(capped.speaker_count(), MAX_SPEAKERS_CAP);
        assert_eq!(cfg.dictation_model, "small");
        assert!(cfg.microphone_name.is_empty());
        assert_eq!(cfg.ui_language, "system");
    }

    /// The settings drawer clamps the same value before it ever reaches Rust.
    /// If the two caps drift, the UI offers a speaker count the backend silently
    /// reduces.
    #[test]
    fn speaker_cap_matches_the_frontend() {
        let ts = std::fs::read_to_string("../src/lib/configStore.ts").expect("read configStore.ts");
        let line = ts
            .lines()
            .find(|l| l.contains("export const MAX_SPEAKERS ="))
            .expect("MAX_SPEAKERS declaration in configStore.ts");
        let value: u8 = line
            .split('=')
            .nth(1)
            .and_then(|v| v.trim().trim_end_matches(';').parse().ok())
            .expect("numeric MAX_SPEAKERS");
        assert_eq!(
            value, MAX_SPEAKERS_CAP,
            "MAX_SPEAKERS_CAP in config.rs and MAX_SPEAKERS in src/lib/configStore.ts have drifted"
        );
    }

    /// The frontend applies the same rule before it decides the Markdown would
    /// be empty; the two disagreeing means one side offers a run the other
    /// refuses. The cases live in the TS test and are read from there.
    #[test]
    fn local_endpoint_matches_the_frontend() {
        let ts = std::fs::read_to_string("../src/lib/llmProviders.test.ts")
            .expect("read llmProviders.test.ts");
        let block = ts
            .split_once("LOCAL_ENDPOINT_CASES: Array<[string, boolean]> = [")
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(list, _)| list)
            .expect("LOCAL_ENDPOINT_CASES in llmProviders.test.ts");

        let mut checked = 0;
        for line in block.lines() {
            let line = line.trim().trim_end_matches(',');
            let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) else {
                continue;
            };
            let (url, expected) = inner.rsplit_once(',').expect("pair");
            let url = url.trim().trim_matches('"');
            let expected: bool = expected.trim().parse().expect("bool");
            assert_eq!(is_local_endpoint(url), expected, "{url}");
            checked += 1;
        }
        assert!(checked >= 8, "only {checked} cases parsed");
    }

    #[test]
    fn local_endpoints_need_no_api_key() {
        let ollama = AppConfig {
            api_key: String::new(),
            api_base_url: "http://localhost:11434/v1".to_string(),
            api_model: "llama3.1".to_string(),
            ..AppConfig::default()
        };
        assert!(ollama.summary_enabled());
        assert!(ollama.validate_for_run().is_ok());

        let remote = AppConfig {
            api_key: String::new(),
            ..AppConfig::default()
        };
        assert!(!remote.summary_enabled());
    }

    #[test]
    fn summary_without_key_and_no_transcript_is_rejected() {
        let cfg = AppConfig {
            include_transcript: false,
            ..AppConfig::default()
        };
        let err = cfg.validate_for_run().unwrap_err();
        assert!(err.contains("No API key"));
    }

    #[test]
    fn validate_rejects_empty_output() {
        let cfg = AppConfig {
            api_key: "k".into(),
            include_summary: false,
            include_transcript: false,
            ..AppConfig::default()
        };
        let err = cfg.validate_for_run().unwrap_err();
        assert!(err.contains("Summary or Transcript"));
    }

    /// Exercises the locale→ISO step directly instead of going through
    /// `sys_locale`. Setting `LANG` was both racy (the test harness is
    /// multi-threaded, and the write leaked into every other test) and unreliable:
    /// `sys_locale` reads `LANGUAGE`, `LC_ALL` and `LC_MESSAGES` first, so the
    /// assertion failed on any machine where one of those was set.
    #[test]
    fn normalize_locale_to_iso639_1() {
        assert_eq!(normalize_iso639_1("de_DE.UTF-8"), Some("de".to_string()));
        assert_eq!(normalize_iso639_1("de-DE"), Some("de".to_string()));
        assert_eq!(normalize_iso639_1("EN"), Some("en".to_string()));
        assert_eq!(normalize_iso639_1("pt_BR"), Some("pt".to_string()));
        assert_eq!(normalize_iso639_1("C"), None);
        assert_eq!(normalize_iso639_1(""), None);
        assert_eq!(normalize_iso639_1("42_XX"), None);
    }

    /// `system` must always resolve to something usable, whatever the host locale.
    #[test]
    fn resolve_system_yields_an_iso_code() {
        let lang = resolve_summary_language("system");
        assert_eq!(lang.len(), 2, "expected an ISO 639-1 code, got {lang:?}");
        assert!(lang.chars().all(|c| c.is_ascii_lowercase()));
    }
}
