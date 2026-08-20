use std::path::{Path, PathBuf};

use crate::audio::SAMPLE_RATE;
use crate::llm::{format_transcript, speakers_for_lines, TranscriptLine};
use crate::model_download;

const SEG_URL: &str =
    "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx";
const EMB_URL: &str = "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx";
const SEG_FILE: &str = "segmentation-3.0.onnx";
const EMB_FILE: &str = "wespeaker_en_voxceleb_CAM++.onnx";
const SIMILARITY: f32 = 0.5;
const AUTO_SPEAKER_CAP: usize = 20;

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxmd")
        .join("diarize")
}

fn cached(name: &str) -> PathBuf {
    cache_dir().join(name)
}

/// Downloads the two ONNX models if they are not already in the cache.
pub async fn ensure_models(
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<(), String> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Diarize cache: {e}"))?;
    let cb = std::sync::Arc::new(on_progress);
    for (url, name) in [(SEG_URL, SEG_FILE), (EMB_URL, EMB_FILE)] {
        let dest = cached(name);
        if dest.is_file() && dest.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            continue;
        }
        let cb = cb.clone();
        model_download::download_file(url, &dest, move |d, t| cb(d, t)).await?;
    }
    Ok(())
}

fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// Runs pyannote segmentation + embeddings and labels Whisper lines.
///
/// Isolated in this module so a later engine swap does not touch the pipeline.
pub fn label_transcript(
    samples_f32: &[f32],
    lines: &[TranscriptLine],
    max_speakers: u8,
) -> Result<String, String> {
    if lines.is_empty() {
        return Ok(String::new());
    }
    let seg_path = cached(SEG_FILE);
    let emb_path = cached(EMB_FILE);
    if !seg_path.is_file() || !emb_path.is_file() {
        return Err("Diarization models are not cached. They download on first use.".to_string());
    }
    let turns = diarize_turns(samples_f32, &seg_path, &emb_path, max_speakers)?;
    let assigned = speakers_for_lines(lines, &turns);
    Ok(format_transcript(lines, &assigned))
}

fn diarize_turns(
    samples_f32: &[f32],
    seg_path: &Path,
    emb_path: &Path,
    max_speakers: u8,
) -> Result<Vec<(f32, f32, usize)>, String> {
    let samples = to_i16(samples_f32);
    let cap = if max_speakers == 0 {
        AUTO_SPEAKER_CAP
    } else {
        (max_speakers as usize).clamp(1, AUTO_SPEAKER_CAP)
    };

    let mut extractor = pyannote_rs::EmbeddingExtractor::new(emb_path)
        .map_err(|e| format!("Diarization embedding model: {e}"))?;
    let mut manager = pyannote_rs::EmbeddingManager::new(cap);
    let segments = pyannote_rs::get_segments(&samples, SAMPLE_RATE, seg_path)
        .map_err(|e| format!("Diarization segmentation: {e}"))?;

    let mut turns = Vec::new();
    for segment in segments {
        let seg = segment.map_err(|e| format!("Diarization segment: {e}"))?;
        let embedding = extractor
            .compute(&seg.samples)
            .map_err(|e| format!("Diarization embedding: {e}"))?;
        let vec: Vec<f32> = embedding.collect();
        let Some(id) = manager.search_speaker(vec, SIMILARITY) else {
            continue;
        };
        // EmbeddingManager assigns 1-based ids (see pyannote-rs EmbeddingManager::new).
        turns.push((seg.start as f32, seg.end as f32, id));
    }
    Ok(turns)
}

#[cfg(test)]
mod tests {
    use super::to_i16;

    #[test]
    fn f32_to_i16_clamps() {
        let out = to_i16(&[0.0, 1.0, -1.0, 2.0]);
        assert_eq!(out, vec![0, 32767, -32767, 32767]);
    }
}
