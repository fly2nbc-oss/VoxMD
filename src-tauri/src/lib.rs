mod audio;
mod config;
mod diarize;
mod dictation;
mod llm;
mod meta;
mod model_download;
mod pipeline;
mod podcast;
mod vulkan_runtime;

use config::AppConfig;
use dictation::MicrophoneInfo;
use llm::LlmModelInfo;
use model_download::ModelInfo;
use podcast::{EpisodeInfo, QueueItem};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VulkanStatus {
    /// Compiled with Cargo feature `gpu-vulkan`.
    built_with_vulkan: bool,
    /// System Vulkan loader (`libvulkan.so.1` / `vulkan-1.dll`) can be opened.
    loader_available: bool,
    /// GPU Whisper path is usable right now (`built_with_vulkan && loader_available`).
    available: bool,
}

#[tauri::command]
fn processing_state() -> bool {
    pipeline::is_processing()
}

#[tauri::command(async)]
fn vulkan_status() -> VulkanStatus {
    let built_with_vulkan = vulkan_runtime::built_with_vulkan();
    let loader_available = vulkan_runtime::loader_available();
    VulkanStatus {
        built_with_vulkan,
        loader_available,
        available: built_with_vulkan && loader_available,
    }
}

/// Returns available Whisper model names with cache status.
///
/// The `async` marker on this and the commands below only moves them off the
/// main thread — they touch the filesystem, `dlopen` the Vulkan loader or join a
/// capture thread, any of which freezes the window when run inline.
#[tauri::command(async)]
fn list_whisper_models() -> Vec<ModelInfo> {
    model_download::list_models()
}

/// Returns the local cache directory for Whisper models.
#[tauri::command(async)]
fn whisper_cache_dir() -> String {
    model_download::cache_dir().to_string_lossy().into_owned()
}

/// Deletes all cached Whisper model files.
#[tauri::command(async)]
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
async fn list_llm_models(config: AppConfig) -> Result<Vec<LlmModelInfo>, String> {
    llm::list_llm_models(&config).await
}

#[tauri::command]
async fn verify_api_key(config: AppConfig) -> Result<(), String> {
    llm::verify_api_key(&config).await
}

#[tauri::command]
async fn improve_text(config: AppConfig, text: String) -> Result<String, String> {
    llm::improve_text(&config, &text).await
}

#[tauri::command]
async fn translate_text(config: AppConfig, text: String, target: String) -> Result<String, String> {
    llm::translate_text(&config, &text, &target).await
}

#[tauri::command(async)]
fn list_microphones() -> Result<Vec<MicrophoneInfo>, String> {
    dictation::list_microphones()
}

#[tauri::command(async)]
fn start_mic_monitor(app: tauri::AppHandle, microphone_name: String) -> Result<(), String> {
    dictation::start_monitor(app, microphone_name)
}

#[tauri::command(async)]
fn stop_mic_monitor() {
    dictation::stop_monitor();
}

#[tauri::command]
async fn start_dictation(app: tauri::AppHandle, config: AppConfig) -> Result<(), String> {
    dictation::start(app, config).await
}

#[tauri::command]
fn stop_dictation() {
    dictation::stop();
}

#[tauri::command]
fn dictation_state() -> bool {
    dictation::is_running()
}

#[tauri::command]
fn append_to_batch(items: Vec<QueueItem>) -> Result<usize, String> {
    pipeline::append_to_batch(items)
}

#[tauri::command]
async fn start_transcription(
    app: tauri::AppHandle,
    items: Vec<QueueItem>,
    config: AppConfig,
) -> Result<(), String> {
    config.validate_for_run()?;
    // Claim the slot before returning, so the frontend cannot enable its Cancel
    // button while the flag is still unset. `run_batch` releases it via its guard
    // and reports the outcome through the `batch_complete` event.
    //
    // Claiming *before* the dictation check closes the window in which both
    // could start: `dictation::start` tests `is_processing()` after its own
    // compare-exchange, so whichever claims first wins and the loser backs out.
    pipeline::begin_batch()?;
    if dictation::is_running() {
        pipeline::release_batch();
        return Err("Stop dictation before starting a batch.".to_string());
    }
    pipeline::enqueue_items(items);
    tokio::spawn(async move {
        pipeline::run_batch(app, config).await;
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
            append_to_batch,
            processing_state,
            vulkan_status,
            list_whisper_models,
            whisper_cache_dir,
            clear_whisper_cache,
            system_summary_language,
            fetch_podcast_feed,
            list_llm_models,
            verify_api_key,
            improve_text,
            translate_text,
            list_microphones,
            start_mic_monitor,
            stop_mic_monitor,
            start_dictation,
            stop_dictation,
            dictation_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VoxMD");
}
