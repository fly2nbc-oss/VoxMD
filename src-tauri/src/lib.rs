mod audio;
mod config;
mod llm;
mod meta;
mod model_download;
mod pipeline;
mod podcast;

use config::AppConfig;
use model_download::ModelInfo;
use podcast::{EpisodeInfo, QueueItem};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VulkanStatus {
    built_with_vulkan: bool,
}

#[tauri::command]
fn processing_state() -> bool {
    pipeline::is_processing()
}

#[tauri::command]
fn vulkan_status() -> VulkanStatus {
    VulkanStatus {
        built_with_vulkan: cfg!(feature = "gpu-vulkan"),
    }
}

/// Returns available Whisper model names with cache status.
#[tauri::command]
fn list_whisper_models() -> Vec<ModelInfo> {
    model_download::list_models()
}

/// Returns the local cache directory for Whisper models.
#[tauri::command]
fn whisper_cache_dir() -> String {
    model_download::cache_dir().to_string_lossy().into_owned()
}

/// Deletes all cached Whisper model files.
#[tauri::command]
fn clear_whisper_cache() -> Result<(), String> {
    model_download::clear_model_cache()
}

#[tauri::command]
fn cancel_transcription() {
    pipeline::request_cancel();
}

/// Resolved ISO 639-1 code when summary language is set to `system`.
#[tauri::command]
fn system_summary_language() -> String {
    config::resolve_summary_language("system")
}

/// Loads an RSS/Atom feed and returns its episodes (audio enclosures only).
#[tauri::command]
async fn fetch_podcast_feed(url: String) -> Result<Vec<EpisodeInfo>, String> {
    podcast::fetch_feed(&url).await
}

#[tauri::command]
async fn start_transcription(
    app: tauri::AppHandle,
    items: Vec<QueueItem>,
    config: AppConfig,
) -> Result<(), String> {
    config.validate_for_run()?;
    tokio::spawn(async move {
        let _ = pipeline::run_batch(app, items, config).await;
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            start_transcription,
            cancel_transcription,
            processing_state,
            vulkan_status,
            list_whisper_models,
            whisper_cache_dir,
            clear_whisper_cache,
            system_summary_language,
            fetch_podcast_feed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VoxMD");
}
