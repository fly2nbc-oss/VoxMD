use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use std::path::{Path, PathBuf};
pub const AUDIO_EXTENSIONS: &[&str] =
    &["mp3", "m4a", "mp4", "wav", "ogg", "flac", "webm", "opus"];

/// Entfernt ungültige Windows-Zeichen analog zum Python-Skript.
pub fn sanitize_filename(filename: &str) -> String {
    let invalid = r#"<>:\"/\\|?*"#;
    filename
        .chars()
        .map(|c| if invalid.contains(c) { '_' } else { c })
        .collect::<String>()
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_string()
}

pub fn get_audio_metadata(audio_path: &Path) -> (String, Option<String>) {
    let fallback = audio_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_string();

    let Ok(tagged) = lofty::read_from_path(audio_path) else {
        return (fallback, None);
    };

    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let Some(tag) = tag else {
        return (fallback, None);
    };

    let title = tag
        .title()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| fallback.clone());

    let year = tag.year().map(|y| y.to_string());

    (title, year)
}

pub fn get_md_path(audio_path: &Path, title: &str, year: &Option<String>) -> PathBuf {
    let stem = match (
        year.clone(),
        title != audio_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(""),
    ) {
        (Some(y), _) if !title.is_empty() => format!("{y} - {title}"),
        (None, true) if !title.is_empty() => title.to_string(),
        _ => audio_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio")
            .to_string(),
    };

    let base = audio_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(sanitize_filename(&stem));
    base.with_extension("md")
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_lowercase();
            AUDIO_EXTENSIONS.iter().any(|ext| *ext == e.as_str())
        })
        .unwrap_or(false)
}
