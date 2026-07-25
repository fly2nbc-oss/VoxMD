use std::io::Write;
use std::path::Path;
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::meta::AUDIO_EXTENSIONS;

/// One entry in the processing queue: a local audio file or a podcast episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    /// Stable key: local path or episode audio URL (mirrors the frontend list key).
    pub id: String,
    /// "local" | "podcast"
    pub kind: String,
    /// Local file path or episode audio URL.
    pub source: String,
    pub display_name: String,
    #[serde(default)]
    pub episode: Option<EpisodeMeta>,
}

impl QueueItem {
    pub fn is_podcast(&self) -> bool {
        self.kind == "podcast"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeMeta {
    pub feed_title: String,
    pub title: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    /// Target folder for the generated `.md` (podcast episodes have no local source).
    pub output_dir: String,
}

/// Episode as returned by `fetch_podcast_feed` (frontend turns these into `QueueItem`s).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeInfo {
    pub feed_title: String,
    pub title: String,
    pub date: Option<String>,
    pub link: Option<String>,
    pub audio_url: String,
}

pub async fn fetch_feed(url: &str) -> Result<Vec<EpisodeInfo>, String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Feed URL must start with http:// or https://.".to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent("VoxMD")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Feed request: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Feed request failed: HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Feed download: {e}"))?;

    let feed = feed_rs::parser::parse(&bytes[..])
        .map_err(|e| format!("Not a valid RSS/Atom feed: {e}"))?;

    let feed_title = feed
        .title
        .map(|t| t.content.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Podcast".to_string());

    let mut episodes = Vec::new();
    for entry in feed.entries {
        let Some(audio_url) = pick_audio_url(&entry) else {
            continue;
        };
        let title = entry
            .title
            .map(|t| t.content.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Episode".to_string());
        let date = entry
            .published
            .or(entry.updated)
            .map(|d| d.to_rfc3339()[..10].to_string());
        let link = entry
            .links
            .iter()
            .find(|l| l.rel.as_deref() != Some("enclosure"))
            .map(|l| l.href.clone());

        episodes.push(EpisodeInfo {
            feed_title: feed_title.clone(),
            title,
            date,
            link,
            audio_url,
        });
    }

    if episodes.is_empty() {
        return Err("Feed contains no episodes with an audio enclosure.".to_string());
    }
    Ok(episodes)
}

/// Audio URL of an entry: media content first (RSS enclosures land there), link fallback.
fn pick_audio_url(entry: &feed_rs::model::Entry) -> Option<String> {
    for media in &entry.media {
        for content in &media.content {
            let Some(u) = &content.url else { continue };
            let mime_is_audio = content
                .content_type
                .as_ref()
                .map(|m| m.to_string().starts_with("audio/"))
                .unwrap_or(false);
            if mime_is_audio || has_audio_extension(u.path()) {
                return Some(u.to_string());
            }
        }
    }
    entry
        .links
        .iter()
        .find(|l| {
            l.rel.as_deref() == Some("enclosure")
                && (l
                    .media_type
                    .as_deref()
                    .map(|m| m.starts_with("audio/"))
                    .unwrap_or(false)
                    || has_audio_extension(&l.href))
        })
        .map(|l| l.href.clone())
}

fn has_audio_extension(path: &str) -> bool {
    let lower = path.to_lowercase();
    AUDIO_EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// File extension hint for the temp download (Symphonia probes content, but a hint helps).
fn url_extension(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        ext
    } else {
        "mp3".to_string()
    }
}

/// Download an episode into a self-deleting temp file.
///
/// Sync wrapper for the blocking Whisper task; must be called from a thread with a
/// Tokio runtime context (e.g. inside `spawn_blocking`).
pub fn download_to_temp_blocking(
    url: &str,
    on_progress: impl Fn(u64, u64),
) -> Result<tempfile::NamedTempFile, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| "No async runtime available for episode download.".to_string())?;
    handle.block_on(download_to_temp(url, on_progress))
}

async fn download_to_temp(
    url: &str,
    on_progress: impl Fn(u64, u64),
) -> Result<tempfile::NamedTempFile, String> {
    let client = reqwest::Client::builder()
        .user_agent("VoxMD")
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Episode download: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Episode download failed: HTTP {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut tmp = tempfile::Builder::new()
        .prefix("voxmd-episode-")
        .suffix(&format!(".{}", url_extension(url)))
        .tempfile()
        .map_err(|e| format!("Create temp file: {e}"))?;

    let mut downloaded = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Episode download stream: {e}"))?;
        tmp.as_file_mut()
            .write_all(&chunk)
            .map_err(|e| format!("Write temp file: {e}"))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    tmp.as_file_mut()
        .flush()
        .map_err(|e| format!("Flush temp file: {e}"))?;

    Ok(tmp)
}

#[cfg(test)]
mod tests {
    use super::{has_audio_extension, url_extension};

    #[test]
    fn url_extension_strips_query_and_falls_back() {
        assert_eq!(url_extension("https://cdn.example.com/ep1.mp3?dl=1"), "mp3");
        assert_eq!(url_extension("https://cdn.example.com/ep1.m4a"), "m4a");
        assert_eq!(url_extension("https://cdn.example.com/stream"), "mp3");
    }

    #[test]
    fn audio_extension_detection() {
        assert!(has_audio_extension("/podcast/ep.OGG"));
        assert!(!has_audio_extension("/podcast/cover.jpg"));
    }
}
