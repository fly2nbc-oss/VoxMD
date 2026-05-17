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
    /// Pfad zur GGUF/GGML-Modell-Datei für whisper.cpp
    pub whisper_model_path: String,
    #[serde(default)]
    pub whisper_threads: Option<usize>,
    #[serde(default = "default_language")]
    pub language: String,
    /// Nutzt GPU, wenn das Binary mit Feature `gpu-vulkan` gebaut wurde
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    #[serde(default)]
    pub delete_source_after_success: bool,
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
            api_base_url: "https://api.deepseek.com/v1".to_string(),
            api_model: "deepseek-chat".to_string(),
            temperature: 0.3,
            max_tokens: 8192,
            transcript_chunk_chars: 32_768,
            whisper_model_path: String::new(),
            whisper_threads: None,
            language: default_language(),
            use_gpu: default_true(),
            delete_source_after_success: false,
        }
    }
}

impl AppConfig {
    pub fn validate_for_run(&self) -> Result<(), String> {
        if self.api_key.trim().is_empty() {
            return Err("API-Key fehlt (Einstellungen).".to_string());
        }
        if self.api_base_url.trim().is_empty() {
            return Err("API Base URL fehlt.".to_string());
        }
        if self.api_model.trim().is_empty() {
            return Err("API Modell fehlt.".to_string());
        }
        if self.whisper_model_path.trim().is_empty() {
            return Err("Whisper-Modellpfad fehlt (lokale GGUF/GGML-Datei).".to_string());
        }
        let p = std::path::Path::new(self.whisper_model_path.trim());
        if !p.is_file() {
            return Err(format!(
                "Whisper-Modell nicht gefunden: {}",
                self.whisper_model_path
            ));
        }
        Ok(())
    }
}
