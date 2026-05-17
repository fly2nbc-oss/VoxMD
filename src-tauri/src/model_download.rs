use std::path::{Path, PathBuf};

use futures::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

const HF_BASE: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// All officially supported whisper.cpp model names → GGUF filename.
pub const MODELS: &[(&str, &str, &str)] = &[
    ("tiny",           "ggml-tiny.bin",               "~75 MB"),
    ("base",           "ggml-base.bin",               "~142 MB"),
    ("small",          "ggml-small.bin",              "~466 MB"),
    ("medium",         "ggml-medium.bin",             "~1.5 GB"),
    ("large-v2",       "ggml-large-v2.bin",           "~3.1 GB"),
    ("large-v3",       "ggml-large-v3.bin",           "~3.1 GB"),
    ("turbo",          "ggml-large-v3-turbo.bin",     "~809 MB"),
    ("large-v3-turbo", "ggml-large-v3-turbo.bin",     "~809 MB"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub name: String,
    pub filename: String,
    pub size_hint: String,
    pub cached: bool,
    pub path: Option<String>,
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxmd")
        .join("whisper")
}

/// Deletes all cached model files from the cache directory.
pub fn clear_model_cache() -> Result<(), String> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("Read cache dir: {e}"))? {
        let entry = entry.map_err(|e| format!("Dir entry: {e}"))?;
        let p = entry.path();
        if p.is_file() {
            std::fs::remove_file(&p).map_err(|e| format!("Delete {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

pub fn list_models() -> Vec<ModelInfo> {
    let dir = cache_dir();
    MODELS
        .iter()
        .filter(|(name, filename, _)| {
            // deduplicate aliases: skip if a prior entry has the same filename
            let idx = MODELS.iter().position(|(_, f, _)| f == filename).unwrap();
            MODELS[idx].0 == *name
        })
        .map(|(name, filename, size_hint)| {
            let p = dir.join(filename);
            let cached = p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false);
            ModelInfo {
                name: name.to_string(),
                filename: filename.to_string(),
                size_hint: size_hint.to_string(),
                cached,
                path: if cached { Some(p.to_string_lossy().into_owned()) } else { None },
            }
        })
        .collect()
}

fn filename_for(name: &str) -> Option<&'static str> {
    MODELS
        .iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, f, _)| *f)
}

/// Resolves `name_or_path` to a local model file, downloading if needed.
///
/// - Existing file path → returned as-is.
/// - Known model name  → cached in `~/.cache/voxmd/whisper/`, downloaded on first use.
/// - `on_progress(downloaded_bytes, total_bytes)` is called during download.
pub async fn resolve_model(
    name_or_path: &str,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<PathBuf, String> {
    let trimmed = name_or_path.trim();
    let p = Path::new(trimmed);
    if p.is_file() {
        return Ok(p.to_path_buf());
    }

    let filename = filename_for(trimmed).ok_or_else(|| {
        format!(
            "Unknown model '{}'. Use a name (turbo, large-v3, medium, small, base, tiny) \
             or a full path to a local .bin / .gguf file.",
            trimmed
        )
    })?;

    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cache dir: {e}"))?;
    let dest = dir.join(filename);

    if dest.is_file() && dest.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(dest);
    }

    let url = format!("{HF_BASE}/{filename}");
    download_file(&url, &dest, on_progress).await?;
    Ok(dest)
}

async fn download_file(
    url: &str,
    dest: &Path,
    on_progress: impl Fn(u64, u64),
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("VoxMD/0.1")
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download '{url}': {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {} – '{url}'",
            resp.status()
        ));
    }

    let total = resp.content_length().unwrap_or(0);
    let tmp = dest.with_extension("tmp");

    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("Create temp file: {e}"))?;

    let mut downloaded = 0u64;
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write: {e}"))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }

    file.flush().await.map_err(|e| format!("Flush: {e}"))?;
    drop(file);

    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| format!("Rename temp file: {e}"))?;

    Ok(())
}
