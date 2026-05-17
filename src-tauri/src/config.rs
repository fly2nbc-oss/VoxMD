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
    /// Use GPU (only effective when built with feature `gpu-vulkan`)
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    #[serde(default = "default_true")]
    pub delete_source_after_success: bool,
    /// Enable whisper.cpp progress/realtime output (WHISPER_CPP_VERBOSE)
    #[serde(default)]
    pub whisper_verbose: bool,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "de".to_string()
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
            use_gpu: default_true(),
            delete_source_after_success: true,
            whisper_verbose: false,
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
