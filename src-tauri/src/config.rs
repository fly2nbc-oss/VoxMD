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

impl AppConfig {
    pub fn validate_for_run(&self) -> Result<(), String> {
        if self.api_key.trim().is_empty() {
            return Err("API key missing (Settings).".to_string());
        }
        if self.api_base_url.trim().is_empty() {
            return Err("API base URL missing.".to_string());
        }
        if self.api_model.trim().is_empty() {
            return Err("API model missing.".to_string());
        }
        if self.whisper_model.trim().is_empty() {
            return Err(
                "Whisper model missing. Enter a name (turbo, large-v3, …) or a local file path."
                    .to_string(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_summary_language;

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
    fn resolve_system_uses_lang_env() {
        // SAFETY: single-threaded test; no concurrent env access.
        unsafe {
            std::env::set_var("LANG", "de_DE.UTF-8");
        }
        assert_eq!(resolve_summary_language("system"), "de");
        assert_eq!(resolve_summary_language("SYSTEM"), "de");
    }
}
