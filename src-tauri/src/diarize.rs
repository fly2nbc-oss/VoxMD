use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use ndarray::{ArrayViewD, Axis};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;

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
const FRAME_SIZE: usize = 270;
const FRAME_START: usize = 721;

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

struct SpeechSeg {
    start: f32,
    end: f32,
    samples: Vec<i16>,
}

/// Runs pyannote segmentation + embeddings and labels Whisper lines.
///
/// Isolated in this module so a later engine swap does not touch the pipeline.
pub fn label_transcript(
    samples_f32: &[f32],
    lines: &[TranscriptLine],
    max_speakers: u8,
    abort: impl Fn() -> bool,
) -> Result<String, String> {
    if lines.is_empty() {
        return Ok(String::new());
    }
    let seg_path = cached(SEG_FILE);
    let emb_path = cached(EMB_FILE);
    if !seg_path.is_file() || !emb_path.is_file() {
        return Err("Diarization models are not cached. They download on first use.".to_string());
    }
    let turns = diarize_turns(samples_f32, &seg_path, &emb_path, max_speakers, &abort)?;
    if turns.is_empty() {
        return Err("No speaker turns found.".to_string());
    }
    let assigned = speakers_for_lines(lines, &turns);
    Ok(format_transcript(lines, &assigned))
}

fn diarize_turns(
    samples_f32: &[f32],
    seg_path: &Path,
    emb_path: &Path,
    max_speakers: u8,
    abort: &impl Fn() -> bool,
) -> Result<Vec<(f32, f32, usize)>, String> {
    if samples_f32.is_empty() {
        return Err("No audio for diarization.".to_string());
    }
    let samples = to_i16(samples_f32);
    let cap = if max_speakers == 0 {
        AUTO_SPEAKER_CAP
    } else {
        (max_speakers as usize).clamp(1, AUTO_SPEAKER_CAP)
    };

    let mut extractor = pyannote_rs::EmbeddingExtractor::new(emb_path)
        .map_err(|e| format!("Diarization embedding model: {e}"))?;
    let mut manager = pyannote_rs::EmbeddingManager::new(cap);
    let segments = speech_segments(&samples, SAMPLE_RATE, seg_path, abort)?;

    let mut turns = Vec::new();
    for seg in segments {
        if abort() {
            return Err("Cancelled.".to_string());
        }
        if seg.samples.is_empty() {
            continue;
        }
        let embedding = match extractor.compute(&seg.samples) {
            Ok(iter) => iter.collect::<Vec<f32>>(),
            Err(_) => continue,
        };
        let at_cap = manager.get_all_speakers().len() >= cap;
        let id = if at_cap {
            match manager.get_best_speaker_match(embedding) {
                Ok(id) => id,
                Err(_) => continue,
            }
        } else {
            match manager.search_speaker(embedding, SIMILARITY) {
                Some(id) => id,
                None => continue,
            }
        };
        // EmbeddingManager assigns 1-based ids (see pyannote-rs EmbeddingManager::new).
        turns.push((seg.start, seg.end, id));
    }
    Ok(turns)
}

/// Local copy of pyannote-rs 0.3.4 `get_segments` with three upstream bugs fixed:
/// 1. i16 samples must be scaled to [-1, 1] or the ONNX model classifies every
///    frame as non-speech (thewh1teagle/pyannote-rs#28).
/// 2. Speech that lasts until EOF was never flushed as a segment.
/// 3. The original `from_fn` stopped when a 10 s window produced no *closed*
///    segment (typical for a long opening utterance), dropping the rest.
fn speech_segments(
    samples: &[i16],
    sample_rate: u32,
    model_path: &Path,
    abort: &impl Fn() -> bool,
) -> Result<Vec<SpeechSeg>, String> {
    let mut session = seg_session(model_path)?;
    let window_size = (sample_rate as usize).saturating_mul(10);
    if window_size == 0 {
        return Err("Invalid sample rate for diarization.".to_string());
    }

    let mut padded = samples.to_vec();
    let rem = samples.len() % window_size;
    padded.extend(std::iter::repeat_n(0, window_size - rem));

    let mut is_speeching = false;
    let mut offset = FRAME_START;
    let mut start_offset = 0.0_f64;
    let mut out = Vec::new();

    for start in (0..padded.len()).step_by(window_size) {
        if abort() {
            return Err("Cancelled.".to_string());
        }
        let end = (start + window_size).min(padded.len());
        let window = &padded[start..end];
        let samples_n = ndarray::Array1::from_iter(window.iter().map(|&x| x as f32 / 32768.0));
        let view = samples_n.view().insert_axis(Axis(0)).insert_axis(Axis(1));
        let inputs = ort::inputs![TensorRef::from_array_view(view.into_dyn())
            .map_err(|e| format!("Diarization input: {e}"))?];
        let ort_outs = session
            .run(inputs)
            .map_err(|e| format!("Diarization segmentation: {e}"))?;
        let ort_out = ort_outs
            .get("output")
            .ok_or_else(|| "Diarization segmentation: output tensor missing".to_string())?;
        let (shape, data) = ort_out
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Diarization segmentation: {e}"))?;
        let shape_slice: Vec<usize> = (0..shape.len()).map(|i| shape[i] as usize).collect();
        let view = ArrayViewD::<f32>::from_shape(ndarray::IxDyn(&shape_slice), data)
            .map_err(|e| format!("Diarization segmentation: {e}"))?;

        for row in view.outer_iter() {
            for sub_row in row.axis_iter(Axis(0)) {
                let max_index = argmax(sub_row.iter().copied())?;
                if max_index != 0 {
                    if !is_speeching {
                        start_offset = offset as f64;
                        is_speeching = true;
                    }
                } else if is_speeching {
                    is_speeching = false;
                    out.push(make_seg(
                        start_offset,
                        offset as f64,
                        sample_rate,
                        samples.len(),
                        &padded,
                    ));
                }
                offset += FRAME_SIZE;
            }
        }
    }

    if is_speeching {
        out.push(make_seg(
            start_offset,
            offset as f64,
            sample_rate,
            samples.len(),
            &padded,
        ));
    }
    Ok(out)
}

fn make_seg(
    start_offset: f64,
    end_offset: f64,
    sample_rate: u32,
    samples_len: usize,
    padded: &[i16],
) -> SpeechSeg {
    let rate = sample_rate as f64;
    let start = start_offset / rate;
    let end = end_offset / rate;
    let last = samples_len.saturating_sub(1);
    let start_idx = ((start * rate) as usize).min(last);
    let end_idx = ((end * rate) as usize).min(samples_len);
    let slice = if start_idx < end_idx {
        &padded[start_idx..end_idx]
    } else {
        &[]
    };
    SpeechSeg {
        start: start as f32,
        end: end as f32,
        samples: slice.to_vec(),
    }
}

fn argmax(values: impl IntoIterator<Item = f32>) -> Result<usize, String> {
    let mut best_i = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    let mut any = false;
    for (i, v) in values.into_iter().enumerate() {
        any = true;
        if v.partial_cmp(&best_v).unwrap_or(Ordering::Equal) == Ordering::Greater {
            best_v = v;
            best_i = i;
        }
    }
    if any {
        Ok(best_i)
    } else {
        Err("Diarization segmentation: empty frame".to_string())
    }
}

fn seg_session(path: &Path) -> Result<Session, String> {
    Session::builder()
        .map_err(|e| format!("Diarization session: {e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| format!("Diarization session: {e}"))?
        .with_intra_threads(1)
        .map_err(|e| format!("Diarization session: {e}"))?
        .with_inter_threads(1)
        .map_err(|e| format!("Diarization session: {e}"))?
        .commit_from_file(path)
        .map_err(|e| format!("Diarization session: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{argmax, make_seg, to_i16};

    #[test]
    fn f32_to_i16_clamps() {
        let out = to_i16(&[0.0, 1.0, -1.0, 2.0]);
        assert_eq!(out, vec![0, 32767, -32767, 32767]);
    }

    #[test]
    fn argmax_picks_largest() {
        assert_eq!(argmax([0.1, 0.9, 0.2]).unwrap(), 1);
    }

    #[test]
    fn make_seg_clamps_to_original_length() {
        let padded = vec![1_i16, 2, 3, 4, 0, 0];
        let seg = make_seg(0.0, 4.0, 2, 4, &padded);
        assert_eq!(seg.samples, vec![1, 2, 3, 4]);
        assert!((seg.start - 0.0).abs() < f32::EPSILON);
        assert!((seg.end - 2.0).abs() < f32::EPSILON);
    }
}
