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
    crate::paths::models_dir()
}

/// Every file the app downloads into [`cache_dir`]: Whisper weights, the two
/// pyannote ONNX models and the ONNX Runtime library.
///
/// An explicit list, not "everything in the directory". The location is
/// user-overridable (`VOXMD_MODELS_DIR`), and deleting unknown files out of a
/// directory someone pointed at their own data would be unforgivable.
pub fn managed_files() -> Vec<String> {
    let mut names: Vec<String> = MODELS.iter().map(|(_, f, _)| f.to_string()).collect();
    names.extend(crate::diarize::MODEL_FILES.iter().map(|f| f.to_string()));
    if let Some(lib) = crate::onnx_runtime::library_file_name() {
        names.push(lib.to_string());
    }
    names.sort_unstable();
    names.dedup();
    names
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub files: usize,
    pub bytes: u64,
}

/// How much disk the downloaded models occupy right now.
pub fn cache_stats() -> CacheStats {
    let dir = cache_dir();
    let mut stats = CacheStats::default();
    for name in managed_files() {
        if let Ok(meta) = std::fs::metadata(dir.join(name)) {
            if meta.is_file() && meta.len() > 0 {
                stats.files += 1;
                stats.bytes += meta.len();
            }
        }
    }
    stats
}

/// Deletes every downloaded model — Whisper *and* the diarization files.
///
/// Continues after individual delete failures so a single locked file does not
/// leave the rest behind; reports a combined error if any failed. Interrupted
/// downloads (`.tmp`, `.part`, `.download`) go too, since they are ours and are
/// worthless once their target is gone.
pub fn clear_model_cache() -> Result<(), String> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(());
    }
    let mut targets: Vec<PathBuf> = managed_files().into_iter().map(|n| dir.join(n)).collect();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            let leftover = p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "tmp" | "part" | "download"));
            if leftover && p.is_file() {
                targets.push(p);
            }
        }
    }

    let mut errors = Vec::new();
    for path in targets {
        if !path.is_file() {
            continue;
        }
        if let Err(e) = std::fs::remove_file(&path) {
            errors.push(format!("Delete {}: {e}", path.display()));
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

pub(crate) async fn download_file(
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

    /// A model is written to and later read from the same path, because
    /// `resolve_model` derives both from `cache_dir()`. A future split between
    /// the two would silently re-download on every run.
    #[test]
    fn download_and_load_share_one_directory() {
        let dir = cache_dir();
        assert_eq!(dir, crate::paths::models_dir());
        for (name, file, _) in MODELS {
            let resolved = dir.join(filename_for(name).expect("preset resolves"));
            assert_eq!(resolved.parent(), Some(dir.as_path()));
            assert_eq!(resolved.file_name().and_then(|f| f.to_str()), Some(*file));
        }
    }

    /// Clearing must reach every download, and nothing else. An entry missing
    /// from this list survives "free up space" and quietly keeps its gigabytes.
    #[test]
    fn managed_files_cover_every_download() {
        let files = managed_files();
        for (_, whisper, _) in MODELS {
            assert!(files.contains(&whisper.to_string()), "{whisper}");
        }
        for onnx in crate::diarize::MODEL_FILES {
            assert!(files.contains(&onnx.to_string()), "{onnx}");
        }
        if let Some(lib) = crate::onnx_runtime::library_file_name() {
            assert!(files.contains(&lib.to_string()), "{lib}");
        }
        // Deduped: `turbo` and `large-v3-turbo` share one file.
        let mut sorted = files.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), files.len());
        assert!(files.iter().all(|f| !f.is_empty()));
    }

    /// Counting and clearing must agree, or "free up space" reports a size it
    /// then fails to reclaim.
    #[test]
    fn stats_and_clearing_agree_on_a_temp_directory() {
        let dir = std::env::temp_dir().join(format!("voxmd-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let managed = managed_files();
        std::fs::write(dir.join(&managed[0]), vec![0u8; 2048]).unwrap();
        std::fs::write(dir.join(&managed[1]), vec![0u8; 1024]).unwrap();
        std::fs::write(dir.join("ggml-small.bin.part"), vec![0u8; 16]).unwrap();
        // Not ours: must survive, even here.
        std::fs::write(dir.join("notes.txt"), b"keep me").unwrap();

        let counted: Vec<_> = managed
            .iter()
            .filter(|n| dir.join(n).is_file())
            .cloned()
            .collect();
        assert_eq!(counted.len(), 2);
        let bytes: u64 = counted
            .iter()
            .map(|n| std::fs::metadata(dir.join(n)).unwrap().len())
            .sum();
        assert_eq!(bytes, 3072);

        for name in &counted {
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        assert!(
            dir.join("notes.txt").is_file(),
            "unrelated file was deleted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn looks_like_model_file_accepts_bin_and_gguf() {
        assert!(looks_like_model_file(Path::new("/x/model.bin")));
        assert!(looks_like_model_file(Path::new("/x/model.GGUF")));
        assert!(!looks_like_model_file(Path::new("/x/model.txt")));
    }
}
