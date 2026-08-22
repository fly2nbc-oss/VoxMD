use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ndarray::{ArrayViewD, Axis};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;

use crate::audio::SAMPLE_RATE;
use crate::config::MAX_SPEAKERS_CAP;
use crate::llm::{format_transcript, smooth_speaker_outliers, speakers_for_lines, TranscriptLine};
use crate::model_download;
use crate::onnx_runtime;

const SEG_URL: &str =
    "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx";
const EMB_URL: &str = "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM++.onnx";
const SEG_FILE: &str = "segmentation-3.0.onnx";
const EMB_FILE: &str = "wespeaker_en_voxceleb_CAM++.onnx";
/// Auto mode stops at this many speakers even if the distance threshold would
/// keep more (pyannote-rs used 20 and routinely invented extras).
const AUTO_SPEAKER_CAP: usize = MAX_SPEAKERS_CAP as usize;
const FRAME_SIZE: usize = 270;
const FRAME_START: usize = 721;

/// Merge same-class speech across a pause shorter than this, within one window.
const MERGE_GAP_S: f32 = 0.25;
/// CAM++ embeddings below this duration are noise; skip them.
const MIN_EMBED_S: f32 = 0.4;
/// Anchors for clustering: long enough for a stable voiceprint, no overlap class.
const MIN_ANCHOR_S: f32 = 1.5;
const MAX_ANCHORS: usize = 600;
/// Average-linkage cosine distance below which two clusters merge in auto mode.
/// Distance 0.45 ≈ cosine similarity 0.55.
const CLUSTER_DIST: f32 = 0.45;
/// Drop clusters whose total anchored speech is shorter than this (auto mode).
const MIN_CLUSTER_S: f32 = 3.0;
/// Turns shorter than this are absorbed into the longer neighbour.
const MIN_TURN_S: f32 = 0.4;

/// `(start_s, end_s, speaker_id)` — 1-based after `polish_turns`.
type SpeakerTurn = (f32, f32, usize);

/// Powerset classes 4–6 are two simultaneous speakers inside a 10 s window.
fn is_overlap_class(class: usize) -> bool {
    class >= 4
}

/// Absolute sample position of frame `frame` in the window starting at
/// `window_start`. The grid is anchored to each window — see bug 5 on
/// [`speech_segments`].
fn frame_offset(window_start: usize, frame: usize) -> usize {
    window_start + FRAME_START + frame * FRAME_SIZE
}

/// Everything diarization needs at runtime — both ONNX models *and* the ONNX
/// Runtime shared library — is downloaded into and loaded from this one
/// directory. `cached()` is the only way to name a file in it.
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxmd")
        .join("diarize")
}

fn cached(name: &str) -> PathBuf {
    cache_dir().join(name)
}

/// Downloads the two ONNX models and the ONNX Runtime library if they are not
/// already cached. `on_progress(step, of, downloaded, total)` reports per file,
/// since each is a separate request with its own content length.
pub async fn ensure_models(
    on_progress: impl Fn(usize, usize, u64, u64) + Send + Sync + 'static,
) -> Result<(), String> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Diarize cache: {e}"))?;
    let cb = std::sync::Arc::new(on_progress);
    const STEPS: usize = 3;

    for (i, (url, name)) in [(SEG_URL, SEG_FILE), (EMB_URL, EMB_FILE)]
        .into_iter()
        .enumerate()
    {
        let dest = cached(name);
        if dest.is_file() && dest.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            continue;
        }
        let cb = cb.clone();
        model_download::download_file(url, &dest, move |d, t| cb(i + 1, STEPS, d, t)).await?;
    }

    let cb = cb.clone();
    onnx_runtime::ensure(&dir, move |d, t| cb(STEPS, STEPS, d, t)).await
}

/// True once every diarization asset is present in [`cache_dir`].
pub fn models_cached() -> bool {
    cached(SEG_FILE).is_file()
        && cached(EMB_FILE).is_file()
        && onnx_runtime::is_cached(&cache_dir())
}

fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// One stretch of speech. `range` indexes the *original* (unpadded) sample
/// buffer; holding indices instead of an owned copy keeps a multi-hour episode
/// from allocating a second full-length i16 buffer spread over thousands of Vecs.
struct SpeechSeg {
    start: f32,
    end: f32,
    range: (usize, usize),
    class: u8,
    overlap: bool,
    window: usize,
}

impl SpeechSeg {
    fn duration(&self) -> f32 {
        (self.end - self.start).max(0.0)
    }

    fn samples<'a>(&self, all: &'a [i16]) -> &'a [i16] {
        let (a, b) = self.range;
        let a = a.min(all.len());
        let b = b.clamp(a, all.len());
        &all[a..b]
    }
}

struct SegEmb {
    start: f32,
    end: f32,
    embedding: Option<Vec<f32>>,
    overlap: bool,
}

impl SegEmb {
    fn duration(&self) -> f32 {
        (self.end - self.start).max(0.0)
    }
    fn mid(&self) -> f32 {
        0.5 * (self.start + self.end)
    }
}

#[allow(dead_code)]
struct DiarizeStats {
    n_segments: usize,
    duration_hist: [usize; 5],
    n_with_embed: usize,
    n_anchors: usize,
    merge_distances: Vec<f32>,
    n_clusters: usize,
    speech_per_cluster: Vec<f32>,
    first_turns: Vec<SpeakerTurn>,
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
    if !models_cached() {
        return Err("Diarization models are not cached. They download on first use.".to_string());
    }
    let (turns, _) = run_diarize(samples_f32, &seg_path, &emb_path, max_speakers, &abort)?;
    if turns.is_empty() {
        return Err("No speaker turns found.".to_string());
    }
    let mut assigned = speakers_for_lines(lines, &turns);
    smooth_speaker_outliers(lines, &mut assigned);
    Ok(format_transcript(lines, &assigned))
}

fn run_diarize(
    samples_f32: &[f32],
    seg_path: &Path,
    emb_path: &Path,
    max_speakers: u8,
    abort: &impl Fn() -> bool,
) -> Result<(Vec<SpeakerTurn>, DiarizeStats), String> {
    if samples_f32.is_empty() {
        return Err("No audio for diarization.".to_string());
    }
    // Before the first Session: `ort` panics rather than erroring when it cannot
    // open its dylib, so the path is validated here.
    onnx_runtime::init(&cache_dir())?;
    let samples = to_i16(samples_f32);
    let target_k = if max_speakers == 0 {
        None
    } else {
        Some((max_speakers as usize).clamp(1, AUTO_SPEAKER_CAP))
    };

    let mut extractor = pyannote_rs::EmbeddingExtractor::new(emb_path)
        .map_err(|e| format!("Diarization embedding model: {e}"))?;
    let segments = speech_segments(&samples, SAMPLE_RATE, seg_path, abort)?;
    let duration_hist = duration_histogram(segments.iter().map(SpeechSeg::duration));

    let mut items = Vec::with_capacity(segments.len());
    for seg in &segments {
        if abort() {
            return Err("Cancelled.".to_string());
        }
        let seg_samples = seg.samples(&samples);
        let embedding = if seg.duration() >= MIN_EMBED_S && !seg_samples.is_empty() {
            match extractor.compute(seg_samples) {
                Ok(iter) => {
                    let mut v: Vec<f32> = iter.collect();
                    l2_normalize(&mut v);
                    Some(v)
                }
                Err(_) => None,
            }
        } else {
            None
        };
        items.push(SegEmb {
            start: seg.start,
            end: seg.end,
            embedding,
            overlap: seg.overlap,
        });
    }

    let (turns, stats) = cluster_and_assign(&items, target_k, duration_hist)?;
    Ok((turns, stats))
}

fn cluster_and_assign(
    items: &[SegEmb],
    target_k: Option<usize>,
    duration_hist: [usize; 5],
) -> Result<(Vec<SpeakerTurn>, DiarizeStats), String> {
    let n_with_embed = items.iter().filter(|s| s.embedding.is_some()).count();
    let mut anchor_idx: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, s)| s.embedding.is_some() && !s.overlap && s.duration() >= MIN_ANCHOR_S)
        .map(|(i, _)| i)
        .collect();

    if anchor_idx.is_empty() {
        // Fall back to any embedded segment so a short clip still labels.
        let mut any: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, s)| s.embedding.is_some())
            .map(|(i, _)| i)
            .collect();
        any.sort_by(|a, b| {
            items[*b]
                .duration()
                .partial_cmp(&items[*a].duration())
                .unwrap_or(Ordering::Equal)
        });
        any.truncate(MAX_ANCHORS);
        anchor_idx = any;
    } else if anchor_idx.len() > MAX_ANCHORS {
        anchor_idx.sort_by(|a, b| {
            items[*b]
                .duration()
                .partial_cmp(&items[*a].duration())
                .unwrap_or(Ordering::Equal)
        });
        anchor_idx.truncate(MAX_ANCHORS);
    }

    if anchor_idx.is_empty() {
        return Err("No speaker turns found.".to_string());
    }

    let embeddings: Vec<Vec<f32>> = anchor_idx
        .iter()
        .map(|&i| items[i].embedding.clone().unwrap_or_default())
        .collect();
    let durations: Vec<f32> = anchor_idx.iter().map(|&i| items[i].duration()).collect();

    let (anchor_labels, merge_distances) =
        agglomerative_cluster(&embeddings, CLUSTER_DIST, target_k, AUTO_SPEAKER_CAP);

    let keep = if target_k.is_none() {
        keep_cluster_mask(&anchor_labels, &durations, MIN_CLUSTER_S)
    } else {
        vec![true; anchor_labels.iter().copied().max().unwrap_or(0) + 1]
    };

    let centroids = centroids_from_kept(&embeddings, &anchor_labels, &keep);
    if centroids.is_empty() {
        return Err("No speaker turns found.".to_string());
    }

    let mut seg_labels: Vec<Option<usize>> = vec![None; items.len()];
    for (i, item) in items.iter().enumerate() {
        let Some(emb) = item.embedding.as_deref() else {
            continue;
        };
        seg_labels[i] = Some(nearest_centroid(emb, &centroids));
    }
    fill_nearest_in_time(&mut seg_labels, items);

    let mut turns: Vec<SpeakerTurn> = items
        .iter()
        .zip(seg_labels.iter())
        .filter_map(|(item, lab)| lab.map(|l| (item.start, item.end, l)))
        .collect();
    turns = polish_turns(turns);

    let mut acc: HashMap<usize, f32> = HashMap::new();
    for &(start, end, id) in &turns {
        *acc.entry(id).or_insert(0.0) += (end - start).max(0.0);
    }
    let mut ids: Vec<usize> = acc.keys().copied().collect();
    ids.sort_unstable();
    let speech_per_cluster: Vec<f32> = ids.into_iter().map(|id| acc[&id]).collect();

    let first_turns = turns.iter().copied().take(40).collect();
    let stats = DiarizeStats {
        n_segments: items.len(),
        duration_hist,
        n_with_embed,
        n_anchors: anchor_idx.len(),
        merge_distances,
        n_clusters: speech_per_cluster.len(),
        speech_per_cluster,
        first_turns,
    };
    Ok((turns, stats))
}

fn duration_histogram(durs: impl IntoIterator<Item = f32>) -> [usize; 5] {
    let mut hist = [0usize; 5];
    for d in durs {
        let slot = if d < MIN_EMBED_S {
            0
        } else if d < MIN_ANCHOR_S {
            1
        } else if d < 5.0 {
            2
        } else if d < 10.0 {
            3
        } else {
            4
        };
        hist[slot] += 1;
    }
    hist
}

fn l2_normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-12 {
        let inv = 1.0 / n;
        for x in v {
            *x *= inv;
        }
    }
}

fn cosine_dist(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0.0f32;
    for i in 0..n {
        dot += a[i] * b[i];
    }
    1.0 - dot
}

/// Average-linkage agglomerative clustering on L2-normalised embeddings.
///
/// `target_k = Some(k)` cuts the dendrogram at exactly `k` clusters (or `n`
/// if there are fewer points). `None` merges while the closest pair is under
/// `max_dist`, then keeps merging until at most `cap` clusters remain.
///
/// Cluster distances are updated with the Lance-Williams recurrence for UPGMA,
/// `d(i∪j, k) = (nᵢ·d(i,k) + nⱼ·d(j,k)) / (nᵢ+nⱼ)`, which is exact for average
/// linkage. Recomputing each merged distance from the raw pairs instead cost
/// O(n²) per merge and needed a second n×n matrix.
fn agglomerative_cluster(
    embeddings: &[Vec<f32>],
    max_dist: f32,
    target_k: Option<usize>,
    cap: usize,
) -> (Vec<usize>, Vec<f32>) {
    let n = embeddings.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    if n == 1 {
        return (vec![0], Vec::new());
    }

    let mut live = vec![true; n];
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut cdist = vec![0.0f32; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            let d = cosine_dist(&embeddings[i], &embeddings[j]);
            cdist[i * n + j] = d;
            cdist[j * n + i] = d;
        }
    }
    let mut n_live = n;
    let mut merges = Vec::new();
    let floor = target_k.unwrap_or(1).clamp(1, n);

    loop {
        if n_live <= floor {
            break;
        }
        let mut best = f32::INFINITY;
        let mut bi = 0usize;
        let mut bj = 0usize;
        for i in 0..n {
            if !live[i] {
                continue;
            }
            for j in (i + 1)..n {
                if !live[j] {
                    continue;
                }
                let d = cdist[i * n + j];
                if d < best {
                    best = d;
                    bi = i;
                    bj = j;
                }
            }
        }
        if !best.is_finite() {
            break;
        }
        if target_k.is_none() && n_live <= cap && best >= max_dist {
            break;
        }

        merges.push(best);
        let (wi, wj) = (members[bi].len() as f32, members[bj].len() as f32);
        let total = wi + wj;
        for k in 0..n {
            if !live[k] || k == bi || k == bj {
                continue;
            }
            let d = (wi * cdist[bi * n + k] + wj * cdist[bj * n + k]) / total;
            cdist[bi * n + k] = d;
            cdist[k * n + bi] = d;
        }

        let other = std::mem::take(&mut members[bj]);
        live[bj] = false;
        n_live -= 1;
        members[bi].extend(other);
    }

    let mut labels = vec![0usize; n];
    let mut next = 0usize;
    for (i, live_i) in live.iter().enumerate() {
        if !live_i {
            continue;
        }
        let id = next;
        next += 1;
        for &p in &members[i] {
            labels[p] = id;
        }
    }
    (labels, merges)
}

fn keep_cluster_mask(labels: &[usize], durations: &[f32], min_s: f32) -> Vec<bool> {
    let k = labels.iter().copied().max().unwrap_or(0) + 1;
    let mut tot = vec![0.0f32; k];
    for (&lab, &d) in labels.iter().zip(durations.iter()) {
        if lab < k {
            tot[lab] += d;
        }
    }
    let mut keep: Vec<bool> = tot.iter().map(|&t| t >= min_s).collect();
    if !keep.iter().any(|&kept| kept) {
        if let Some((i, _)) = tot
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(Ordering::Equal))
        {
            keep[i] = true;
        }
    }
    keep
}

fn centroids_from_kept(embeddings: &[Vec<f32>], labels: &[usize], keep: &[bool]) -> Vec<Vec<f32>> {
    let dim = embeddings.first().map(|e| e.len()).unwrap_or(0);
    if dim == 0 {
        return Vec::new();
    }
    let k = keep.len();
    let mut sums = vec![vec![0.0f32; dim]; k];
    let mut counts = vec![0u32; k];
    for (emb, &lab) in embeddings.iter().zip(labels.iter()) {
        if lab >= k || !keep[lab] {
            continue;
        }
        counts[lab] += 1;
        for (s, x) in sums[lab].iter_mut().zip(emb.iter()) {
            *s += *x;
        }
    }
    let mut centroids = Vec::new();
    for (i, (sum, &c)) in sums.iter().zip(counts.iter()).enumerate() {
        if c == 0 || !keep[i] {
            continue;
        }
        let mut v: Vec<f32> = sum.iter().map(|x| x / c as f32).collect();
        l2_normalize(&mut v);
        centroids.push(v);
    }
    centroids
}

fn nearest_centroid(emb: &[f32], centroids: &[Vec<f32>]) -> usize {
    let mut best = 0usize;
    let mut best_d = f32::MAX;
    for (i, c) in centroids.iter().enumerate() {
        let d = cosine_dist(emb, c);
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

fn fill_nearest_in_time(labels: &mut [Option<usize>], items: &[SegEmb]) {
    let labeled: Vec<usize> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.map(|_| i))
        .collect();
    if labeled.is_empty() {
        return;
    }
    for i in 0..labels.len() {
        if labels[i].is_some() {
            continue;
        }
        let mut best = labeled[0];
        let mut best_d = f32::MAX;
        for &j in &labeled {
            let d = interval_gap(items[i].start, items[i].end, items[j].start, items[j].end)
                .min((items[i].mid() - items[j].mid()).abs());
            if d < best_d {
                best_d = d;
                best = j;
            }
        }
        labels[i] = labels[best];
    }
}

fn interval_gap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    if a1 < b0 {
        b0 - a1
    } else if b1 < a0 {
        a0 - b1
    } else {
        0.0
    }
}

fn polish_turns(mut turns: Vec<SpeakerTurn>) -> Vec<SpeakerTurn> {
    if turns.is_empty() {
        return turns;
    }
    turns.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
    turns = merge_adjacent_turns(turns);
    turns = absorb_short_turns(turns, MIN_TURN_S);
    turns = merge_adjacent_turns(turns);
    renumber_by_first_appearance(&mut turns);
    turns
}

fn merge_adjacent_turns(turns: Vec<SpeakerTurn>) -> Vec<SpeakerTurn> {
    let mut out: Vec<SpeakerTurn> = Vec::with_capacity(turns.len());
    for t in turns {
        if let Some(last) = out.last_mut() {
            if last.2 == t.2 {
                last.1 = last.1.max(t.1);
                continue;
            }
        }
        out.push(t);
    }
    out
}

fn absorb_short_turns(mut turns: Vec<SpeakerTurn>, min_s: f32) -> Vec<SpeakerTurn> {
    let mut i = 0;
    while i < turns.len() {
        if turns.len() == 1 {
            break;
        }
        let dur = (turns[i].1 - turns[i].0).max(0.0);
        if dur >= min_s {
            i += 1;
            continue;
        }
        let prev_dur = if i > 0 {
            (turns[i - 1].1 - turns[i - 1].0).max(0.0)
        } else {
            0.0
        };
        let next_dur = if i + 1 < turns.len() {
            (turns[i + 1].1 - turns[i + 1].0).max(0.0)
        } else {
            0.0
        };
        let merge_into_prev = i > 0 && (i + 1 == turns.len() || prev_dur >= next_dur);
        if merge_into_prev {
            turns[i - 1].1 = turns[i - 1].1.max(turns[i].1);
            turns.remove(i);
            if i < turns.len() && turns[i - 1].2 == turns[i].2 {
                turns[i - 1].1 = turns[i - 1].1.max(turns[i].1);
                turns.remove(i);
            }
        } else if i + 1 < turns.len() {
            turns[i + 1].0 = turns[i + 1].0.min(turns[i].0);
            turns.remove(i);
            if i > 0 && turns[i - 1].2 == turns[i].2 {
                turns[i - 1].1 = turns[i - 1].1.max(turns[i].1);
                turns.remove(i);
                i = i.saturating_sub(1);
            }
        } else {
            i += 1;
        }
    }
    turns
}

fn renumber_by_first_appearance(turns: &mut [SpeakerTurn]) {
    let mut map = HashMap::new();
    let mut next = 1usize;
    for t in turns.iter_mut() {
        let id = *map.entry(t.2).or_insert_with(|| {
            let id = next;
            next += 1;
            id
        });
        t.2 = id;
    }
}

/// Local copy of pyannote-rs 0.3.4 `get_segments` with upstream bugs fixed,
/// plus a cut on every powerset class change and at each 10 s window boundary.
///
/// 1. i16 samples must be scaled to [-1, 1] or the ONNX model classifies every
///    frame as non-speech (thewh1teagle/pyannote-rs#28).
/// 2. Speech that lasts until EOF is flushed.
/// 3. The original `from_fn` stopped when a 10 s window produced no *closed*
///    segment (typical for a long opening utterance), dropping the rest.
/// 4. Local speaker indices are only valid inside one window, so a turn never
///    spans a window boundary. Overlap classes (4–6) are marked, not treated
///    as a third speaker.
/// 5. The frame counter is re-based to each window. Upstream carries it across
///    windows, but the model emits 589 frames of 270 samples for a 160000-sample
///    window, so the counter falls 970 samples (~61 ms) behind per window —
///    about 22 s over an hour, which silently shifts every later speaker turn.
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

    if samples.is_empty() {
        return Ok(Vec::new());
    }
    // The model wants a full 10 s window. Only the trailing partial window needs
    // zero padding, so pad into a small scratch buffer instead of cloning the
    // whole (multi-hundred-MB) episode.
    let mut tail = Vec::new();

    let mut cur_class: Option<usize> = None;
    let mut start_offset = 0.0_f64;
    let mut start_window = 0usize;
    let mut out = Vec::new();

    for (window_i, start) in (0..samples.len()).step_by(window_size).enumerate() {
        if abort() {
            return Err("Cancelled.".to_string());
        }
        // The frame grid restarts with every window. See bug 5 above.
        let mut frame = 0usize;
        let mut offset = frame_offset(start, frame);
        let end = (start + window_size).min(samples.len());
        let window = if end - start == window_size {
            &samples[start..end]
        } else {
            tail.clear();
            tail.extend_from_slice(&samples[start..end]);
            tail.resize(window_size, 0);
            &tail[..]
        };
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
                let speech_class = if max_index == 0 {
                    None
                } else {
                    Some(max_index)
                };
                match (cur_class, speech_class) {
                    (None, None) => {}
                    (None, Some(c)) => {
                        start_offset = offset as f64;
                        start_window = window_i;
                        cur_class = Some(c);
                    }
                    (Some(c), Some(d)) if c == d => {}
                    (Some(c), new) => {
                        out.push(make_seg(
                            start_offset,
                            offset as f64,
                            sample_rate,
                            samples.len(),
                            c,
                            start_window,
                        ));
                        if let Some(d) = new {
                            start_offset = offset as f64;
                            start_window = window_i;
                            cur_class = Some(d);
                        } else {
                            cur_class = None;
                        }
                    }
                }
                frame += 1;
                offset = frame_offset(start, frame);
            }
        }

        // Local speaker indices do not carry across the 10 s window.
        if let Some(c) = cur_class.take() {
            out.push(make_seg(
                start_offset,
                offset as f64,
                sample_rate,
                samples.len(),
                c,
                start_window,
            ));
        }
    }

    Ok(merge_short_gaps(out))
}

fn merge_short_gaps(segs: Vec<SpeechSeg>) -> Vec<SpeechSeg> {
    let mut out: Vec<SpeechSeg> = Vec::with_capacity(segs.len());
    for seg in segs {
        if let Some(last) = out.last_mut() {
            let gap = seg.start - last.end;
            if last.window == seg.window
                && last.class == seg.class
                && (0.0..MERGE_GAP_S).contains(&gap)
            {
                last.end = seg.end;
                // Contiguous range: the sub-250 ms pause is kept rather than
                // spliced out, which also avoids a discontinuity in the audio
                // the embedder sees.
                last.range.1 = seg.range.1.max(last.range.1);
                continue;
            }
        }
        out.push(seg);
    }
    out
}

fn make_seg(
    start_offset: f64,
    end_offset: f64,
    sample_rate: u32,
    samples_len: usize,
    class: usize,
    window: usize,
) -> SpeechSeg {
    let rate = sample_rate as f64;
    let start_idx = (start_offset.max(0.0) as usize).min(samples_len);
    let end_idx = (end_offset.max(0.0) as usize).clamp(start_idx, samples_len);
    SpeechSeg {
        start: (start_offset / rate) as f32,
        end: (end_offset / rate) as f32,
        range: (start_idx, end_idx),
        class: class as u8,
        overlap: is_overlap_class(class),
        window,
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

#[allow(dead_code)]
fn format_diarize_report(stats: &DiarizeStats) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "segments: {}  with_embed: {}  anchors: {}  clusters: {}\n",
        stats.n_segments, stats.n_with_embed, stats.n_anchors, stats.n_clusters
    ));
    s.push_str(&format!(
        "duration hist [<0.4, 0.4–1.5, 1.5–5, 5–10, >10]: {:?}\n",
        stats.duration_hist
    ));
    let n = stats.merge_distances.len();
    let preview = stats.merge_distances.iter().take(24);
    s.push_str(&format!("merge distances ({n}):"));
    for d in preview {
        s.push_str(&format!(" {d:.3}"));
    }
    if n > 24 {
        s.push_str(" …");
    }
    s.push('\n');
    s.push_str("speech seconds per speaker:");
    for (i, sec) in stats.speech_per_cluster.iter().enumerate() {
        s.push_str(&format!("  {}: {sec:.1}s", i + 1));
    }
    s.push('\n');
    s.push_str("first turns:\n");
    for (start, end, id) in &stats.first_turns {
        s.push_str(&format!("  [{start:8.2}–{end:8.2}] speaker {id}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec2(x: f32, y: f32) -> Vec<f32> {
        let mut v = vec![x, y];
        l2_normalize(&mut v);
        v
    }

    /// Every runtime asset must be downloaded to *and* loaded from
    /// [`cache_dir`]. The download side derives its destination from `cached()`
    /// / `onnx_runtime::library_path(&cache_dir())`, the load side re-derives it
    /// the same way — this pins the two together so a future asset cannot be
    /// written to one directory and looked up in another.
    #[test]
    fn every_asset_lives_in_the_cache_dir() {
        let dir = cache_dir();
        assert!(
            dir.ends_with("voxmd/diarize") || dir.ends_with("voxmd\\diarize"),
            "{dir:?}"
        );

        let mut assets = vec![cached(SEG_FILE), cached(EMB_FILE)];
        if let Ok(lib) = onnx_runtime::library_path(&dir) {
            assets.push(lib);
        }
        assert_eq!(assets.len(), 3, "onnxruntime has no asset for this target");

        for path in &assets {
            assert_eq!(
                path.parent(),
                Some(dir.as_path()),
                "{path:?} escapes {dir:?}"
            );
            assert!(path.file_name().is_some(), "{path:?}");
        }
        // Distinct names, or one download would clobber another.
        let mut names: Vec<_> = assets.iter().filter_map(|p| p.file_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), assets.len());
    }

    #[test]
    fn f32_to_i16_clamps() {
        let out = to_i16(&[0.0, 1.0, -1.0, 2.0]);
        assert_eq!(out, vec![0, 32767, -32767, 32767]);
    }

    /// segmentation-3.0 emits 589 frames of 270 samples for a 160000-sample
    /// window, so a counter carried across windows falls 970 samples behind each
    /// time. Verified against the ONNX model: output shape `[1, 589, 7]`.
    #[test]
    fn frame_offsets_are_rebased_per_window() {
        const FRAMES_PER_WINDOW: usize = 589;
        let window = SAMPLE_RATE as usize * 10;
        assert_eq!(frame_offset(0, 0), FRAME_START);
        assert_eq!(frame_offset(window, 0), window + FRAME_START);

        let carried = FRAME_START + FRAMES_PER_WINDOW * FRAME_SIZE;
        assert_eq!(frame_offset(window, 0) - carried, 970);
        // ~22 s of drift over an hour of audio if the counter is not re-based.
        assert_eq!((3600 / 10) * 970 / SAMPLE_RATE as usize, 21);
    }

    #[test]
    fn argmax_picks_largest() {
        assert_eq!(argmax([0.1, 0.9, 0.2]).unwrap(), 1);
    }

    #[test]
    fn make_seg_clamps_to_original_length() {
        let all = vec![1_i16, 2, 3, 4];
        // Offsets run past the end of the audio; the range must not.
        let seg = make_seg(0.0, 6.0, 2, 4, 1, 0);
        assert_eq!(seg.samples(&all), &[1, 2, 3, 4]);
        assert!((seg.start - 0.0).abs() < f32::EPSILON);
        assert!((seg.end - 3.0).abs() < f32::EPSILON);
        assert_eq!(seg.class, 1);
        assert!(!seg.overlap);
        assert_eq!(seg.window, 0);
    }

    #[test]
    fn overlap_classes_are_four_through_six() {
        assert!(!is_overlap_class(1));
        assert!(!is_overlap_class(3));
        assert!(is_overlap_class(4));
        assert!(is_overlap_class(6));
        let seg = make_seg(0.0, 2.0, 1, 2, 5, 1);
        assert!(seg.overlap);
    }

    fn seg(start: f32, end: f32, range: (usize, usize), window: usize) -> SpeechSeg {
        SpeechSeg {
            start,
            end,
            range,
            class: 1,
            overlap: false,
            window,
        }
    }

    #[test]
    fn merge_short_gaps_same_window_and_class() {
        let all = vec![1_i16, 2, 3, 4];
        let merged = merge_short_gaps(vec![seg(0.0, 1.0, (0, 2), 0), seg(1.1, 2.0, (3, 4), 0)]);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].end - 2.0).abs() < f32::EPSILON);
        // The range spans the merged pair, gap included.
        assert_eq!(merged[0].samples(&all), &[1, 2, 3, 4]);
    }

    #[test]
    fn merge_short_gaps_skips_other_window() {
        let out = merge_short_gaps(vec![seg(0.0, 1.0, (0, 1), 0), seg(1.1, 2.0, (1, 2), 1)]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn agglomerative_separates_two_blobs() {
        let embs = vec![
            vec2(1.0, 0.0),
            vec2(0.99, 0.02),
            vec2(1.0, 0.03),
            vec2(0.0, 1.0),
            vec2(0.02, 0.99),
            vec2(0.03, 1.0),
        ];
        let (labels, _) = agglomerative_cluster(&embs, CLUSTER_DIST, None, AUTO_SPEAKER_CAP);
        let n = labels.iter().copied().max().unwrap() + 1;
        assert_eq!(n, 2);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[0], labels[2]);
        assert_eq!(labels[3], labels[4]);
        assert_eq!(labels[3], labels[5]);
        assert_ne!(labels[0], labels[3]);
    }

    #[test]
    fn agglomerative_target_k_cuts_exactly() {
        let embs = vec![
            vec2(1.0, 0.0),
            vec2(0.9, 0.1),
            vec2(0.0, 1.0),
            vec2(0.1, 0.9),
            vec2(-1.0, 0.0),
            vec2(-0.9, 0.1),
        ];
        let (labels, _) = agglomerative_cluster(&embs, 0.01, Some(2), AUTO_SPEAKER_CAP);
        let n = labels.iter().copied().max().unwrap() + 1;
        assert_eq!(n, 2);
    }

    #[test]
    fn dissolve_drops_tiny_cluster() {
        let labels = vec![0, 0, 0, 1];
        let durs = vec![5.0, 5.0, 5.0, 0.4];
        let keep = keep_cluster_mask(&labels, &durs, MIN_CLUSTER_S);
        assert!(keep[0]);
        assert!(!keep[1]);
    }

    #[test]
    fn absorb_short_turn_into_neighbours() {
        let turns = vec![(0.0, 5.0, 0), (5.0, 5.2, 1), (5.2, 10.0, 0)];
        let out = polish_turns(turns);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].2, 1);
        assert!((out[0].0 - 0.0).abs() < f32::EPSILON);
        assert!((out[0].1 - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn renumber_follows_first_appearance() {
        let mut turns = vec![(0.0, 1.0, 7), (1.0, 2.0, 3), (2.0, 3.0, 7)];
        renumber_by_first_appearance(&mut turns);
        assert_eq!(turns, vec![(0.0, 1.0, 1), (1.0, 2.0, 2), (2.0, 3.0, 1)]);
    }

    /// Set `VOXMD_DIARIZE_AUDIO` to an episode file to print clustering stats.
    /// Without the variable this is a no-op so the suite stays green.
    #[test]
    fn diarize_report() {
        let Some(path) = std::env::var_os("VOXMD_DIARIZE_AUDIO") else {
            return;
        };
        if path.is_empty() {
            return;
        }
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "VOXMD_DIARIZE_AUDIO is not a file: {}",
            path.display()
        );
        let seg_path = cached(SEG_FILE);
        let emb_path = cached(EMB_FILE);
        assert!(
            models_cached(),
            "diarization assets missing in {}",
            cache_dir().display()
        );
        let samples = crate::audio::decode_file_to_mono_16k(&path, || false)
            .expect("decode audio for diarize_report");
        let max_speakers: u8 = std::env::var("VOXMD_DIARIZE_SPEAKERS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let (_turns, stats) = run_diarize(&samples, &seg_path, &emb_path, max_speakers, &|| false)
            .expect("run diarization");
        eprintln!("{}", format_diarize_report(&stats));
    }
}
