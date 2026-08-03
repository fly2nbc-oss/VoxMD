use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// All officially supported whisper.cpp model names → GGUF filename.
pub const MODELS: &[(&str, &str, &str)] = &[
    ("tiny", "ggml-tiny.bin", "~75 MB"),
    ("base", "ggml-base.bin", "~142 MB"),
    ("small", "ggml-small.bin", "~466 MB"),
    ("medium", "ggml-medium.bin", "~1.5 GB"),
    ("large-v2", "ggml-large-v2.bin", "~3.1 GB"),
    ("large-v3", "ggml-large-v3.bin", "~3.1 GB"),
    ("turbo", "ggml-large-v3-turbo.bin", "~809 MB"),
    ("large-v3-turbo", "ggml-large-v3-turbo.bin", "~809 MB"),
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
///
/// Continues after individual delete failures so a single locked file does not
/// leave the rest of the cache behind; reports a combined error if any failed.
pub fn clear_model_cache() -> Result<(), String> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(());
    }
    let mut errors = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("Read cache dir: {e}"))? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("Dir entry: {e}"));
                continue;
            }
        };
        let p = entry.path();
        if p.is_file() {
            if let Err(e) = std::fs::remove_file(&p) {
                errors.push(format!("Delete {}: {e}", p.display()));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub fn list_models() -> Vec<ModelInfo> {
    let dir = cache_dir();
    MODELS
        .iter()
        .enumerate()
        .filter(|(i, (_, filename, _))| {
            // Keep the first preset per filename (`turbo` before `large-v3-turbo`).
            MODELS
                .iter()
                .position(|(_, f, _)| f == filename)
                .map(|first| first == *i)
                .unwrap_or(false)
        })
        .map(|(_, (name, filename, size_hint))| {
            let p = dir.join(filename);
            let cached = p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false);
            ModelInfo {
                name: name.to_string(),
                filename: filename.to_string(),
                size_hint: size_hint.to_string(),
                cached,
                path: if cached {
                    Some(p.to_string_lossy().into_owned())
                } else {
                    None
                },
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

fn looks_like_model_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    ext == "bin" || ext == "gguf"
}

/// Resolves `name_or_path` to a local model file, downloading if needed.
///
/// - Existing `.bin` / `.gguf` file path → returned as-is.
/// - Known model name  → cached in `~/.cache/voxmd/whisper/`, downloaded on first use.
/// - `on_progress(downloaded_bytes, total_bytes)` is called during download.
pub async fn resolve_model(
    name_or_path: &str,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<PathBuf, String> {
    let trimmed = name_or_path.trim();
    let p = Path::new(trimmed);
    if p.is_file() {
        if !looks_like_model_file(p) {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            return Err(format!(
                "Whisper model must be a .bin or .gguf file, got: .{ext}"
            ));
        }
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
    let tmp = dest.with_extension("tmp");

    // Models are up to ~3 GB; a failed attempt must not leave that much garbage
    // sitting in the cache directory.
    let res = stream_to_temp(url, &tmp, on_progress).await;
    if res.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
        return res;
    }

    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| format!("Rename temp file: {e}"))?;

    Ok(())
}

async fn stream_to_temp(
    url: &str,
    tmp: &Path,
    on_progress: impl Fn(u64, u64),
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(crate::podcast::USER_AGENT)
        // This client previously had no timeout at all, so a half-open connection
        // to HuggingFace wedged the entire batch with no way out but killing the app.
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download '{url}': {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {} – '{url}'", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(tmp)
        .await
        .map_err(|e| format!("Create temp file: {e}"))?;

    let mut downloaded = 0u64;
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        if crate::pipeline::cancel_requested() {
            return Err("Cancelled.".to_string());
        }
        let chunk = chunk.map_err(|e| format!("Stream: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write: {e}"))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }

    file.flush().await.map_err(|e| format!("Flush: {e}"))?;
    drop(file);

    // A truncated body would otherwise be renamed to the final `.bin` and cached
    // forever, failing later with an opaque "Whisper init" error.
    if total > 0 && downloaded != total {
        return Err(format!(
            "Model download incomplete: got {downloaded} of {total} bytes."
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_for_resolves_presets_case_insensitively() {
        assert_eq!(filename_for("turbo"), Some("ggml-large-v3-turbo.bin"));
        assert_eq!(filename_for("TURBO"), Some("ggml-large-v3-turbo.bin"));
        assert_eq!(filename_for("large-v3"), Some("ggml-large-v3.bin"));
        assert!(filename_for("nope").is_none());
    }

    #[test]
    fn list_models_dedupes_turbo_alias() {
        let names: Vec<_> = list_models().into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"turbo".to_string()));
        assert!(!names.contains(&"large-v3-turbo".to_string()));
        assert_eq!(names.iter().filter(|n| *n == "turbo").count(), 1);
    }

    #[test]
    fn looks_like_model_file_accepts_bin_and_gguf() {
        assert!(looks_like_model_file(Path::new("/x/model.bin")));
        assert!(looks_like_model_file(Path::new("/x/model.GGUF")));
        assert!(!looks_like_model_file(Path::new("/x/model.txt")));
    }
}
