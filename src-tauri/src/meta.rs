use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use std::path::{Path, PathBuf};
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "mp4", "wav", "ogg", "flac", "webm", "opus"];

/// Characters that are invalid in a Windows filename. Both separators are here,
/// which is also what keeps a feed-supplied title from escaping its directory.
const INVALID_FILENAME_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Names that address a device rather than a file on Windows.
const RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Leaves room for the extension and a collision suffix inside the 255-byte limit
/// that ext4 and NTFS both impose. Podcast episode titles routinely exceed it.
const MAX_STEM_CHARS: usize = 120;

/// Makes an arbitrary string safe to use as a single filename component.
///
/// Input reaches here straight from RSS feeds and audio tags, so it is treated as
/// untrusted: separators and control characters are replaced, leading/trailing dots
/// and spaces are trimmed, the result is length-capped, and Windows device names
/// are defused.
pub fn sanitize_filename(filename: &str) -> String {
    let cleaned: String = filename
        .chars()
        .map(|c| {
            if INVALID_FILENAME_CHARS.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();

    let mut out: String = cleaned
        .trim_matches(|c: char| c == '.' || c == ' ')
        .chars()
        .take(MAX_STEM_CHARS)
        .collect();
    // Truncation can re-expose a trailing dot or space, which Windows also rejects.
    out = out.trim_end_matches(['.', ' ']).to_string();

    if RESERVED_STEMS.iter().any(|r| {
        out.eq_ignore_ascii_case(r) || out.to_ascii_uppercase().starts_with(&format!("{r}."))
    }) {
        out.push('_');
    }

    out
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

    // lofty 0.24 dropped `Tag::year()` for a full `date()` timestamp, which reads
    // both the recording date and the plain year tag; only the year is used here.
    let year = tag.date().map(|d| d.year.to_string());

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

/// Markdown output path for a podcast episode: `{output_dir}/{YYYY-MM-DD - title}.md`.
pub fn get_md_path_for_episode(output_dir: &Path, title: &str, date: &Option<String>) -> PathBuf {
    output_dir.join(format!("{}.md", episode_stem(title, date)))
}

/// Path this episode would have had before the stem carried the full date.
///
/// Only used to recognise already-processed episodes, so upgrading does not
/// re-transcribe an existing library.
pub fn legacy_md_path_for_episode(
    output_dir: &Path,
    title: &str,
    date: &Option<String>,
) -> PathBuf {
    let year = date
        .as_deref()
        .and_then(|d| d.get(..4))
        .filter(|y| y.chars().all(|c| c.is_ascii_digit()));
    let stem = match (year, title.trim()) {
        (Some(y), t) if !t.is_empty() => format!("{y} - {t}"),
        (_, t) if !t.is_empty() => t.to_string(),
        _ => "episode".to_string(),
    };
    let name = sanitize_filename(&stem);
    let name = if name.is_empty() { "audio" } else { &name };
    output_dir.join(format!("{name}.md"))
}

/// Local audio path for a downloaded episode, sharing the stem of its `.md`.
pub fn get_audio_path_for_episode(
    output_dir: &Path,
    title: &str,
    date: &Option<String>,
    ext: &str,
) -> PathBuf {
    let ext = ext.trim().trim_start_matches('.').to_lowercase();
    let ext = if ext.is_empty() { "mp3" } else { &ext };
    output_dir.join(format!("{}.{ext}", episode_stem(title, date)))
}

/// Sanitized filename stem shared by an episode's `.md` and its audio file.
///
/// Carries the full publication date: with only the year, every episode of a
/// daily show collapsed onto one filename, so all but the first were silently
/// reported as "skipped (exists)".
fn episode_stem(title: &str, date: &Option<String>) -> String {
    let day = date.as_deref().map(str::trim).filter(|d| is_iso_date(d));
    let stem = match (day, title.trim()) {
        (Some(d), t) if !t.is_empty() => format!("{d} - {t}"),
        (None, t) if !t.is_empty() => t.to_string(),
        (Some(d), _) => d.to_string(),
        (None, _) => String::new(),
    };
    let name = sanitize_filename(&stem);
    if name.is_empty() {
        "episode".to_string()
    } else {
        name
    }
}

/// `YYYY-MM-DD`, the shape `podcast::fetch_feed` produces.
fn is_iso_date(d: &str) -> bool {
    d.len() == 10
        && d.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

/// Appends `.md` explicitly — `Path::with_extension` would truncate titles containing dots.
fn md_file_name(stem: &str) -> String {
    let mut name = sanitize_filename(stem);
    if name.is_empty() {
        name = "audio".to_string();
    }
    format!("{name}.md")
}

#[cfg(test)]
mod tests {
    use super::AUDIO_EXTENSIONS;
    use super::{episode_stem, get_audio_path_for_episode, get_md_path_for_episode, is_iso_date};
    use super::{sanitize_filename, MAX_STEM_CHARS};
    use std::path::{Component, Path};

    fn day(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    /// Episode titles come straight from remote feeds, so nothing they contain may
    /// produce anything but a single filename component inside the output folder.
    #[test]
    fn sanitize_never_escapes_its_directory() {
        let hostile = [
            "..",
            "../../etc/passwd",
            "....//....//etc//shadow",
            r"C:\Windows\System32\x",
            r"..\..\secrets",
            "/absolute/path",
            "a/b/c",
            ".",
            "   ...   ",
        ];
        let out = Path::new("/tmp/out");
        for title in hostile {
            let md = get_md_path_for_episode(out, title, &None);
            let parent = md.parent().expect("path has a parent");
            assert_eq!(parent, out, "{title:?} escaped the output folder: {md:?}");
            assert_eq!(
                md.components()
                    .filter(|c| matches!(c, Component::Normal(_)))
                    .count(),
                out.components()
                    .filter(|c| matches!(c, Component::Normal(_)))
                    .count()
                    + 1,
                "{title:?} produced more than one path component: {md:?}"
            );
        }
    }

    #[test]
    fn sanitize_replaces_separators_and_control_chars() {
        assert_eq!(sanitize_filename("a/b"), "a_b");
        assert_eq!(sanitize_filename(r"a\b"), "a_b");
        assert_eq!(sanitize_filename("a\nb\tc"), "a_b_c");
        assert_eq!(sanitize_filename("a\0b"), "a_b");
        assert_eq!(
            sanitize_filename("what? <yes>: \"no\"|*"),
            "what_ _yes__ _no___"
        );
    }

    #[test]
    fn sanitize_trims_dots_and_spaces() {
        assert_eq!(sanitize_filename("  hello  "), "hello");
        assert_eq!(sanitize_filename("...hello..."), "hello");
        assert_eq!(sanitize_filename("..."), "");
        assert_eq!(sanitize_filename(""), "");
    }

    /// ext4 and NTFS both cap a name at 255 bytes; long titles used to fail with a
    /// bare OS error at file-creation time.
    #[test]
    fn sanitize_caps_length() {
        let long = "x".repeat(500);
        assert_eq!(sanitize_filename(&long).chars().count(), MAX_STEM_CHARS);
        // Truncation must not leave a trailing dot behind.
        let dotted = format!("{}....", "y".repeat(MAX_STEM_CHARS - 2));
        assert!(!sanitize_filename(&dotted).ends_with('.'));
    }

    #[test]
    fn sanitize_defuses_windows_device_names() {
        assert_eq!(sanitize_filename("CON"), "CON_");
        assert_eq!(sanitize_filename("nul"), "nul_");
        assert_eq!(sanitize_filename("COM1"), "COM1_");
        assert_eq!(sanitize_filename("CON.txt"), "CON.txt_");
        // Not reserved — must be left alone.
        assert_eq!(sanitize_filename("CONTEXT"), "CONTEXT");
        assert_eq!(sanitize_filename("Console"), "Console");
    }

    /// The regression this whole change exists for: a daily show used to collapse
    /// onto one file per year, so every episode after the first was skipped.
    #[test]
    fn daily_episodes_get_distinct_paths() {
        let out = Path::new("/tmp/out");
        let a = get_md_path_for_episode(out, "Daily Briefing", &day("2026-03-14"));
        let b = get_md_path_for_episode(out, "Daily Briefing", &day("2026-03-15"));
        assert_ne!(a, b);
        assert!(a.ends_with("2026-03-14 - Daily Briefing.md"), "{a:?}");
    }

    /// `delete_source_after_success` pairs the audio with its `.md` by stem; the two
    /// helpers used to disagree on the fallback name ("episode" vs "audio").
    #[test]
    fn md_and_audio_share_one_stem() {
        let out = Path::new("/tmp/out");
        for (title, date) in [
            ("Normal Episode", day("2026-01-02")),
            ("", None),
            ("///", day("2026-01-02")),
            ("...", None),
        ] {
            let md = get_md_path_for_episode(out, title, &date);
            let audio = get_audio_path_for_episode(out, title, &date, "mp3");
            assert_eq!(
                md.file_stem(),
                audio.file_stem(),
                "stems diverged for {title:?}"
            );
        }
    }

    #[test]
    fn episode_stem_falls_back_when_everything_is_unusable() {
        assert_eq!(episode_stem("", &None), "episode");
        assert_eq!(episode_stem("   ", &None), "episode");
        assert_eq!(episode_stem("///", &None), "___");
        assert_eq!(episode_stem("", &day("2026-01-02")), "2026-01-02");
    }

    #[test]
    fn only_well_formed_dates_reach_the_filename() {
        assert!(is_iso_date("2026-01-02"));
        assert!(!is_iso_date("2026-1-2"));
        assert!(!is_iso_date("2026-01-02T10:00:00Z"));
        assert!(!is_iso_date("not-a-date"));
        assert!(!is_iso_date(""));
        // A malformed date must not end up in the name.
        assert_eq!(episode_stem("Title", &day("garbage")), "Title");
    }

    /// The frontend keeps its own copy for the file picker and drag-and-drop.
    /// If the two drift, a format the picker accepts may not be recognised as an
    /// audio enclosure when the same kind of file arrives from a feed.
    #[test]
    fn extension_list_matches_the_frontend() {
        let ts = std::fs::read_to_string("../src/lib/queue.ts").expect("read queue.ts");
        let line = ts
            .lines()
            .find(|l| l.contains("export const AUDIO_EXTENSIONS"))
            .expect("AUDIO_EXTENSIONS declaration in queue.ts");
        let inside = line
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(list, _)| list)
            .expect("bracketed list");
        let frontend: Vec<String> = inside
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let backend: Vec<String> = AUDIO_EXTENSIONS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            backend, frontend,
            "AUDIO_EXTENSIONS in meta.rs and src/lib/queue.ts have drifted"
        );
    }

    #[test]
    fn audio_extension_is_normalised() {
        let out = Path::new("/tmp/out");
        let p = get_audio_path_for_episode(out, "Ep", &None, ".M4A");
        assert!(p.ends_with("Ep.m4a"), "{p:?}");
        let p = get_audio_path_for_episode(out, "Ep", &None, "");
        assert!(p.ends_with("Ep.mp3"), "{p:?}");
    }
}
