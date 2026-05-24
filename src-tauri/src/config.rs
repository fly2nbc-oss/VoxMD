use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub api_key: String,
    pub api_base_url: String,
    pub api_model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub transcript_chunk_chars: usize,
    /// Whisper model: name ("turbo", "large-v3", "medium", "small", "base", "tiny")
    /// or absolute path to a local .bin / .gguf file.
    /// Aliases: whisperModelPath (old store key) → whisperModel.
    #[serde(alias = "whisperModelPath")]
    pub whisper_model: String,
    #[serde(default)]
    pub whisper_threads: Option<usize>,
    #[serde(default = "default_language")]
    pub language: String,
    /// Summary language: `"system"` or ISO 639-1 code (e.g. `de`, `en`).
    #[serde(default = "default_summary_language")]
    pub summary_language: String,
    /// Use GPU (only effective when built with feature `gpu-vulkan`)
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    #[serde(default = "default_true")]
    pub delete_source_after_success: bool,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "de".to_string()
}

fn default_summary_language() -> String {
    "system".to_string()
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
            temperature: 0.7,
            max_tokens: 65_536,
            transcript_chunk_chars: 32_768,
            whisper_model: "turbo".to_string(),
            whisper_threads: None,
            language: default_language(),
            summary_language: default_summary_language(),
            use_gpu: default_true(),
            delete_source_after_success: true,
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
    pub fn validate_for_run(&self) -> Result<(), String> {
        if self.api_key.trim().is_empty() {
            return Err("API key missing (Settings).".to_string());
        }
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
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err("Temperature must be between 0 and 2.".to_string());
        }
        if self.max_tokens == 0 || self.max_tokens > 131_072 {
            return Err("Max tokens must be between 1 and 131072.".to_string());
        }
        if self.transcript_chunk_chars < 256 {
            return Err("Transcript chunk size must be at least 256 characters.".to_string());
        }
        if let Some(t) = self.whisper_threads {
            if t == 0 {
                return Err("Whisper CPU threads must be at least 1.".to_string());
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
    use super::{resolve_summary_language, AppConfig};

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
        let mut cfg = AppConfig::default();
        cfg.api_key = "k".into();
        cfg.api_base_url = "ftp://example.com".into();
        assert!(cfg.validate_for_run().is_err());
    }

    #[test]
    fn validate_custom_whisper_path() {
        let mut cfg = AppConfig::default();
        cfg.api_key = "k".into();
        cfg.whisper_model = "/no/such/model.gguf".into();
        let err = cfg.validate_for_run().unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn resolve_system_uses_lang_env() {
        // SAFETY: single-threaded test; no concurrent env access.
        unsafe {
            std::env::set_var("LANG", "de_DE.UTF-8");
        }
        assert_eq!(resolve_summary_language("system"), "de");
        assert_eq!(resolve_summary_language("SYSTEM"), "de");
    }
}
