mod audio;
mod config;
mod llm;
mod meta;
mod pipeline;

use std::path::PathBuf;

use config::AppConfig;
use meta::is_audio_file;

#[tauri::command]
fn collect_audio_in_directory(dir: String) -> Result<Vec<String>, String> {
    let root = PathBuf::from(dir.trim());
    if !root.is_dir() {
        return Err("Pfad ist kein Ordner.".to_string());
    }
    let mut out: Vec<String> = walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_audio_file(e.path()))
        .map(|e| e.path().to_string_lossy().to_string())
        .collect();
    out.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    Ok(out)
}

#[tauri::command]
fn processing_state() -> bool {
    pipeline::is_processing()
}

#[tauri::command]
async fn start_transcription(
    app: tauri::AppHandle,
    paths: Vec<String>,
    config: AppConfig,
) -> Result<(), String> {
    config.validate_for_run()?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tokio::spawn(async move {
        let _ = pipeline::run_batch(app, paths, config).await;
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
            processing_state,
            collect_audio_in_directory
        ])
        .run(tauri::generate_context!())
        .expect("error while running VoxMD");
}
