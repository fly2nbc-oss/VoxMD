use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use std::path::{Path, PathBuf};
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "mp4", "wav", "ogg", "flac", "webm", "opus"];

/// Strips characters invalid in Windows filenames.
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
        title
            != audio_path
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

    audio_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(md_file_name(&stem))
}

/// Markdown output path for a podcast episode: `{output_dir}/{YYYY - title}.md`.
pub fn get_md_path_for_episode(output_dir: &Path, title: &str, date: &Option<String>) -> PathBuf {
    let year = date
        .as_deref()
        .and_then(|d| d.get(..4))
        .filter(|y| y.chars().all(|c| c.is_ascii_digit()));
    let stem = match year {
        Some(y) if !title.trim().is_empty() => format!("{y} - {title}"),
        _ if !title.trim().is_empty() => title.to_string(),
        _ => "episode".to_string(),
    };
    output_dir.join(md_file_name(&stem))
}

/// Appends `.md` explicitly — `Path::with_extension` would truncate titles containing dots.
fn md_file_name(stem: &str) -> String {
    let mut name = sanitize_filename(stem);
    if name.is_empty() {
        name = "audio".to_string();
    }
    format!("{name}.md")
}
