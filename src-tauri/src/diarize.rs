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
/// segmentation-3.0 encodes up to three concurrent speakers in seven classes:
/// silence, each speaker alone, then each pair.
const POWERSET: [&[usize]; 7] = [&[], &[0], &[1], &[2], &[0, 1], &[0, 2], &[1, 2]];
const LOCAL_SPEAKERS: usize = 3;
/// The six permutations of three local speakers, for matching one window's
/// arbitrary indices onto the previous window's.
const PERMUTATIONS: [[usize; LOCAL_SPEAKERS]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

/// Merge same-class speech across a pause shorter than this, within one window.
const MERGE_GAP_S: f32 = 0.25;
/// Ceiling for a joined run. Without it, continuous speech chains into one
/// segment spanning minutes — a worse voiceprint, and no turn resolution left.
const MAX_JOINED_S: f32 = 20.0;
/// CAM++ embeddings below this duration are noise; skip them.
const MIN_EMBED_S: f32 = 0.4;
/// Anchors for clustering: long enough for a stable voiceprint, mostly clean.
const MIN_ANCHOR_S: f32 = 1.5;
const MAX_ANCHORS: usize = 600;
/// Average-linkage cosine distance below which two clusters merge in auto mode.
///
/// Measured directly, on stretches whose speaker is known from the transcript:
/// two takes of the same voice land at 0.07–0.18, two different voices at
/// 0.55–0.67. There is a wide empty band between, and 0.40 sits in the middle
/// of it with room on both sides.
///
/// The number is only meaningful together with the segmentation. Tuning it
/// against the old window-chopped segments suggested 0.65 — which is *above*
/// the cross-speaker band and merges everyone into one cluster. Short segments
/// give noisy embeddings; fix the segmentation before touching this.
const CLUSTER_DIST: f32 = 0.40;
/// Drop clusters whose total anchored speech is shorter than this (auto mode).
const MIN_CLUSTER_S: f32 = 3.0;
/// Turns shorter than this are absorbed into the longer neighbour.
const MIN_TURN_S: f32 = 0.4;

/// `(start_s, end_s, speaker_id)` — 1-based after `polish_turns`.
type SpeakerTurn = (f32, f32, usize);

/// Absolute sample position of frame `frame` in the window starting at
/// `window_start`. The grid is anchored to each window — see bug 5 on
/// [`speech_segments`].
fn frame_offset(window_start: usize, frame: usize) -> usize {
    window_start + FRAME_START + frame * FRAME_SIZE
}

/// The ONNX models and the ONNX Runtime library sit in the shared model
/// directory, beside the Whisper models. `cached()` is the only way to name a
/// file in it.
pub fn cache_dir() -> PathBuf {
    crate::paths::models_dir()
}

/// File names this module downloads, for cache accounting and clearing.
pub const MODEL_FILES: &[&str] = &[SEG_FILE, EMB_FILE];

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
    /// Stitched local speaker track (0..[`LOCAL_SPEAKERS`]). Continuous across
    /// window boundaries, but *not* a global speaker: clustering decides that.
    track: usize,
    overlap: bool,
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
    first_turns_all: Vec<SpeakerTurn>,
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
        .filter(|(_, s)| {
            let min = std::env::var("VOXMD_DIARIZE_ANCHOR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(MIN_ANCHOR_S);
            s.embedding.is_some() && !s.overlap && s.duration() >= min
        })
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

    let (anchor_labels, merge_distances) = agglomerative_cluster(
        &embeddings,
        std::env::var("VOXMD_DIARIZE_DIST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(CLUSTER_DIST),
        target_k,
        AUTO_SPEAKER_CAP,
    );

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
        first_turns_all: turns.clone(),
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

/// Runs pyannote segmentation over the whole file and returns speech runs.
///
/// segmentation-3.0 sees 10 s at a time and numbers the speakers it hears
/// *within that window* — index 1 in one window and index 1 in the next are
/// unrelated. The model is designed to be run with OVERLAPPING windows so
/// consecutive windows can be matched up on the part they share; that is what
/// this does, with a 50 % hop and the best of the six index permutations.
///
/// The previous version stepped window by window and simply cut every turn at
/// the boundary, because it had no way to carry identity across. That produced
/// speaker changes in lockstep with the 10 s grid — 62 % of all turn boundaries
/// landed on a multiple of ten, where chance is about 4 %. A continuous
/// monologue came out as alternating speakers.
///
/// Upstream bugs fixed along the way (pyannote-rs 0.3.4 `get_segments`):
/// 1. i16 samples must be scaled to [-1, 1] or every frame reads as non-speech
///    (thewh1teagle/pyannote-rs#28).
/// 2. Speech lasting until EOF is flushed rather than dropped.
/// 3. The original `from_fn` stopped when a window produced no *closed*
///    segment — typical for a long opening utterance — losing the rest.
/// 4. The frame counter is anchored per window (see [`frame_offset`]).
fn speech_segments(
    samples: &[i16],
    sample_rate: u32,
    model_path: &Path,
    abort: &impl Fn() -> bool,
) -> Result<Vec<SpeechSeg>, String> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let mut session = seg_session(model_path)?;
    let window_size = (sample_rate as usize).saturating_mul(10);
    if window_size == 0 {
        return Err("Invalid sample rate for diarization.".to_string());
    }

    // Global frame grid: frame g starts at sample FRAME_START + g * FRAME_SIZE.
    // Hopping by a whole number of frames keeps every window on that grid, so
    // a window's local frame f is simply global frame `base + f`.
    let mut tracks: Vec<[bool; LOCAL_SPEAKERS]> = Vec::new();
    let mut hop_frames = 0usize;
    let mut base = 0usize;
    let mut window_start = 0usize;
    let mut scratch: Vec<i16> = Vec::new();

    while window_start < samples.len() {
        if abort() {
            return Err("Cancelled.".to_string());
        }
        let end = (window_start + window_size).min(samples.len());
        let window: &[i16] = if end - window_start == window_size {
            &samples[window_start..end]
        } else {
            scratch.clear();
            scratch.extend_from_slice(&samples[window_start..end]);
            scratch.resize(window_size, 0);
            &scratch
        };

        let activity = window_activity(&mut session, window)?;
        if activity.is_empty() {
            break;
        }
        if hop_frames == 0 {
            // Learned from the model rather than hardcoded: 589 frames for a
            // 160000-sample window, so a 50 % hop is 294 frames.
            hop_frames = (activity.len() / 2).max(1);
        }

        // Match this window's arbitrary indices onto what the overlap already
        // holds, then merge. Chained window to window, so a run stays on one
        // track for as long as the speech itself continues.
        let perm = best_permutation(&tracks, &activity, base);
        if tracks.len() < base + activity.len() {
            tracks.resize(base + activity.len(), [false; LOCAL_SPEAKERS]);
        }
        for (f, frame) in activity.iter().enumerate() {
            for local in 0..LOCAL_SPEAKERS {
                if frame[perm[local]] {
                    tracks[base + f][local] = true;
                }
            }
        }

        if end == samples.len() {
            break;
        }
        window_start += hop_frames * FRAME_SIZE;
        base += hop_frames;
    }

    Ok(merge_short_gaps(runs_from_tracks(
        &tracks,
        sample_rate,
        samples.len(),
    )))
}

/// One window through the model, decoded from powerset classes to per-frame
/// activity for each of the three local speakers.
fn window_activity(
    session: &mut Session,
    window: &[i16],
) -> Result<Vec<[bool; LOCAL_SPEAKERS]>, String> {
    let samples_n = ndarray::Array1::from_iter(window.iter().map(|&x| x as f32 / 32768.0));
    let view = samples_n.view().insert_axis(Axis(0)).insert_axis(Axis(1));
    let inputs = ort::inputs![TensorRef::from_array_view(view.into_dyn())
        .map_err(|e| format!("Diarization input: {e}"))?];
    let outs = session
        .run(inputs)
        .map_err(|e| format!("Diarization segmentation: {e}"))?;
    let out = outs
        .get("output")
        .ok_or_else(|| "Diarization segmentation: output tensor missing".to_string())?;
    let (shape, data) = out
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("Diarization segmentation: {e}"))?;
    let dims: Vec<usize> = (0..shape.len()).map(|i| shape[i] as usize).collect();
    let view = ArrayViewD::<f32>::from_shape(ndarray::IxDyn(&dims), data)
        .map_err(|e| format!("Diarization segmentation: {e}"))?;

    let mut activity = Vec::new();
    for row in view.outer_iter() {
        for frame in row.axis_iter(Axis(0)) {
            let class = argmax(frame.iter().copied())?;
            let mut active = [false; LOCAL_SPEAKERS];
            for &speaker in POWERSET.get(class).copied().unwrap_or(&[]) {
                active[speaker] = true;
            }
            activity.push(active);
        }
    }
    Ok(activity)
}

/// Which relabelling of this window's local speakers best matches the frames
/// the previous window already wrote.
///
/// Scored on frames where both agree a speaker is active; silence carries no
/// information about identity. With nothing to compare against — the first
/// window, or an overlap of pure silence — the identity permutation wins,
/// which is as good a guess as any.
fn best_permutation(
    tracks: &[[bool; LOCAL_SPEAKERS]],
    activity: &[[bool; LOCAL_SPEAKERS]],
    base: usize,
) -> [usize; LOCAL_SPEAKERS] {
    let overlap = tracks.len().saturating_sub(base).min(activity.len());
    if overlap == 0 {
        return PERMUTATIONS[0];
    }
    let mut best = PERMUTATIONS[0];
    let mut best_score = -1i64;
    for perm in PERMUTATIONS {
        let mut score = 0i64;
        for f in 0..overlap {
            for local in 0..LOCAL_SPEAKERS {
                if tracks[base + f][local] && activity[f][perm[local]] {
                    score += 1;
                }
            }
        }
        if score > best_score {
            best_score = score;
            best = perm;
        }
    }
    best
}

/// Contiguous stretches of activity per track, as segments on the sample grid.
fn runs_from_tracks(
    tracks: &[[bool; LOCAL_SPEAKERS]],
    sample_rate: u32,
    samples_len: usize,
) -> Vec<SpeechSeg> {
    let rate = sample_rate as f64;
    let mut out = Vec::new();
    for track in 0..LOCAL_SPEAKERS {
        let mut start: Option<usize> = None;
        for f in 0..=tracks.len() {
            let active = f < tracks.len() && tracks[f][track];
            match (start, active) {
                (None, true) => start = Some(f),
                (Some(from), false) => {
                    out.push(make_run(from, f, track, tracks, rate, samples_len));
                    start = None;
                }
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(Ordering::Equal));
    out
}

fn make_run(
    from: usize,
    to: usize,
    track: usize,
    tracks: &[[bool; LOCAL_SPEAKERS]],
    rate: f64,
    samples_len: usize,
) -> SpeechSeg {
    let start_sample = frame_offset(0, from);
    let end_sample = frame_offset(0, to);
    // Any simultaneous speech at all disqualifies the run as an anchor.
    //
    // Tolerating a little was tried and is much worse: at 5 % the clustering
    // collapsed from two speakers to one. A blended voiceprint sits *between*
    // the two real ones and bridges the clusters, so a handful of clean anchors
    // beats many contaminated ones — everything else is assigned afterwards by
    // nearest centroid anyway.
    let overlap = tracks[from..to]
        .iter()
        .any(|f| f.iter().filter(|a| **a).count() > 1);
    SpeechSeg {
        start: (start_sample as f64 / rate) as f32,
        end: (end_sample as f64 / rate) as f32,
        range: (
            start_sample.min(samples_len),
            end_sample.clamp(start_sample.min(samples_len), samples_len),
        ),
        track,
        overlap,
    }
}

/// Bridges breath-length pauses inside one track.
///
/// Window boundaries no longer split anything — the tracks are stitched across
/// them — so this is only about pauses. The ceiling keeps a long monologue from
/// becoming one segment: a shorter run is a better voiceprint, and turns need
/// somewhere to land.
fn merge_short_gaps(segs: Vec<SpeechSeg>) -> Vec<SpeechSeg> {
    let mut out: Vec<SpeechSeg> = Vec::with_capacity(segs.len());
    for seg in segs {
        if let Some(last) = out.last_mut() {
            let gap = seg.start - last.end;
            if last.track == seg.track
                && (0.0..MERGE_GAP_S).contains(&gap)
                && seg.end - last.start <= MAX_JOINED_S
            {
                last.end = seg.end;
                // The pause is kept rather than spliced out: a discontinuity
                // would be audible to the embedder.
                last.range.1 = seg.range.1.max(last.range.1);
                last.overlap = last.overlap || seg.overlap;
                continue;
            }
        }
        out.push(seg);
    }
    out
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

fn median_turn(turns: &[SpeakerTurn]) -> f32 {
    let mut d: Vec<f32> = turns.iter().map(|t| (t.1 - t.0).max(0.0)).collect();
    if d.is_empty() {
        return 0.0;
    }
    d.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    d[d.len() / 2]
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
    s.push_str(&format!(
        "merge distances ({n}), last 24 (the dendrogram tail):"
    ));
    for d in stats
        .merge_distances
        .iter()
        .rev()
        .take(24)
        .collect::<Vec<_>>()
        .iter()
        .rev()
    {
        s.push_str(&format!(" {d:.3}"));
    }
    s.push('\n');
    // Where would a given threshold have stopped? The count is anchors minus
    // the merges that ran below it.
    s.push_str("clusters if threshold were:");
    for t in [0.45f32, 0.55, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90] {
        let below = stats.merge_distances.iter().filter(|d| **d < t).count();
        s.push_str(&format!(
            "  {t:.2}→{}",
            stats.n_anchors.saturating_sub(below).max(1)
        ));
    }
    s.push('\n');
    s.push_str("speech seconds per speaker:");
    for (i, sec) in stats.speech_per_cluster.iter().enumerate() {
        s.push_str(&format!("  {}: {sec:.1}s", i + 1));
    }
    s.push('\n');
    // How many turn boundaries sit on the 10 s window grid? Speaker changes have
    // no reason to align with the model's input size, so anything above the
    // chance rate is the segmenter showing through instead of the conversation.
    let boundaries: Vec<f32> = stats.first_turns_all.iter().skip(1).map(|t| t.0).collect();
    let on_grid = boundaries
        .iter()
        .filter(|b| {
            let r = *b % 10.0;
            !(0.2..=9.8).contains(&r)
        })
        .count();
    let pct = if boundaries.is_empty() {
        0.0
    } else {
        100.0 * on_grid as f32 / boundaries.len() as f32
    };
    s.push_str(&format!(
        "turns: {}  boundaries on the 10 s grid: {on_grid}/{} ({pct:.0}%)  median turn: {:.1}s\n",
        stats.first_turns_all.len(),
        boundaries.len(),
        median_turn(&stats.first_turns_all)
    ));
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
        // Same directory as the Whisper models: one place, one size, one delete.
        assert_eq!(dir, crate::model_download::cache_dir());

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

    /// The powerset table is what turns a class index into "who is talking".
    /// A wrong entry silently mislabels every overlap.
    #[test]
    fn powerset_decodes_the_seven_classes() {
        assert_eq!(POWERSET[0], &[] as &[usize]);
        assert_eq!(POWERSET[1], &[0]);
        assert_eq!(POWERSET[2], &[1]);
        assert_eq!(POWERSET[3], &[2]);
        // 4..6 are the pairs — simultaneous speech.
        assert_eq!(POWERSET[4], &[0, 1]);
        assert_eq!(POWERSET[5], &[0, 2]);
        assert_eq!(POWERSET[6], &[1, 2]);
        for pair in &POWERSET[4..7] {
            assert_eq!(pair.len(), 2);
        }
    }

    #[test]
    fn permutations_are_the_six_distinct_relabellings() {
        assert_eq!(PERMUTATIONS.len(), 6);
        let mut seen: Vec<[usize; LOCAL_SPEAKERS]> = PERMUTATIONS.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 6);
        for p in PERMUTATIONS {
            let mut sorted = p;
            sorted.sort_unstable();
            assert_eq!(sorted, [0, 1, 2]);
        }
        // The identity must come first: it is the fallback with nothing to match.
        assert_eq!(PERMUTATIONS[0], [0, 1, 2]);
    }

    /// The heart of the stitching. A window's speaker indices are arbitrary, so
    /// the one that continues the previous window has to be found by overlap.
    #[test]
    fn stitching_recovers_a_swapped_window() {
        // Global so far: track 0 talking, track 1 silent.
        let tracks: Vec<[bool; LOCAL_SPEAKERS]> = (0..10).map(|_| [true, false, false]).collect();
        // The next window heard the same voice but numbered it 1.
        let activity: Vec<[bool; LOCAL_SPEAKERS]> = (0..10).map(|_| [false, true, false]).collect();
        // Overlap covers the last 5 frames.
        let perm = best_permutation(&tracks, &activity, 5);
        assert_eq!(perm[0], 1, "track 0 must map onto the window's speaker 1");
    }

    #[test]
    fn stitching_falls_back_to_identity_without_evidence() {
        let silent: Vec<[bool; LOCAL_SPEAKERS]> = (0..8).map(|_| [false; LOCAL_SPEAKERS]).collect();
        let some: Vec<[bool; LOCAL_SPEAKERS]> = (0..8).map(|_| [true, false, false]).collect();
        // No previous frames at all.
        assert_eq!(best_permutation(&[], &some, 0), [0, 1, 2]);
        // An overlap of pure silence says nothing about identity.
        assert_eq!(best_permutation(&silent, &some, 0), [0, 1, 2]);
    }

    #[test]
    fn runs_are_contiguous_activity_per_track() {
        let mut tracks = vec![[false; LOCAL_SPEAKERS]; 10];
        for frame in tracks.iter_mut().take(6).skip(2) {
            frame[0] = true;
        }
        for frame in tracks.iter_mut().take(8).skip(4) {
            frame[1] = true;
        }
        let runs = runs_from_tracks(&tracks, SAMPLE_RATE, usize::MAX);
        assert_eq!(runs.len(), 2);
        // Sorted by start time, and the shared frames 4..6 count as overlap.
        assert_eq!(runs[0].track, 0);
        assert_eq!(runs[1].track, 1);
        assert!(runs[0].overlap && runs[1].overlap);
        assert_eq!(runs[0].range.0, frame_offset(0, 2));
        assert_eq!(runs[0].range.1, frame_offset(0, 6));
    }

    #[test]
    fn a_lone_track_is_not_marked_as_overlap() {
        let mut tracks = vec![[false; LOCAL_SPEAKERS]; 6];
        for frame in tracks.iter_mut().take(4).skip(1) {
            frame[2] = true;
        }
        let runs = runs_from_tracks(&tracks, SAMPLE_RATE, usize::MAX);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].overlap);
        assert_eq!(runs[0].track, 2);
    }

    fn seg(start: f32, end: f32, range: (usize, usize), track: usize) -> SpeechSeg {
        SpeechSeg {
            start,
            end,
            range,
            track,
            overlap: false,
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

    /// Pauses inside one track bridge; a real silence ends the segment.
    #[test]
    fn short_pauses_bridge_within_a_track() {
        let joined = merge_short_gaps(vec![seg(0.0, 9.98, (0, 1), 0), seg(10.05, 19.9, (1, 2), 0)]);
        assert_eq!(joined.len(), 1);
        assert!((joined[0].end - 19.9).abs() < f32::EPSILON);

        // 0.6 s of silence: room for a speaker change, so it stays split.
        let split = merge_short_gaps(vec![seg(0.0, 9.4, (0, 1), 0), seg(10.05, 19.9, (1, 2), 0)]);
        assert_eq!(split.len(), 2);

        // Different tracks never merge, however close.
        let other = merge_short_gaps(vec![seg(0.0, 9.98, (0, 1), 0), seg(10.0, 19.9, (1, 2), 1)]);
        assert_eq!(other.len(), 2);
    }

    /// Without a ceiling, continuous speech chains into one segment spanning
    /// minutes — a worse voiceprint and no turn resolution left.
    #[test]
    fn joining_stops_at_the_length_ceiling() {
        let mut segs = Vec::new();
        let mut t = 0.0f32;
        for _ in 0..8 {
            segs.push(seg(t, t + 9.95, (0, 1), 0));
            t += 10.0;
        }
        let out = merge_short_gaps(segs);
        assert!(out.len() > 1, "everything collapsed into one segment");
        for s in &out {
            assert!(
                s.end - s.start <= MAX_JOINED_S + 0.1,
                "{:.1}s exceeds the ceiling",
                s.end - s.start
            );
        }
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

    /// Runs a real ONNX session whenever the diarization assets are cached.
    ///
    /// This is the test that was missing when `ort`'s `alternative-backend`
    /// feature shipped: everything compiled, every other test passed, CI was
    /// green on both platforms, and the first `Session` panicked. Nothing short
    /// of actually creating one catches that class of mistake, so on any machine
    /// that has used diarization once, it is checked on every `cargo test`.
    ///
    /// Skips loudly where the assets are absent (CI), which is why
    /// `ort_features_let_load_dynamic_initialise_itself` guards the manifest as
    /// well — see the release checklist in CLAUDE.md.
    #[test]
    fn onnx_session_starts_with_the_shipped_features() {
        if !models_cached() {
            eprintln!(
                "skipped: diarization assets not cached in {} — run a diarized batch first",
                cache_dir().display()
            );
            return;
        }
        onnx_runtime::init(&cache_dir()).expect("initialise ONNX Runtime");
        let mut session = seg_session(&cached(SEG_FILE)).expect("open segmentation model");
        let samples = ndarray::Array1::<f32>::zeros(SAMPLE_RATE as usize * 10);
        let view = samples.view().insert_axis(Axis(0)).insert_axis(Axis(1));
        let inputs = ort::inputs![TensorRef::from_array_view(view.into_dyn()).unwrap()];
        let outputs = session.run(inputs).expect("run segmentation");
        let (shape, _) = outputs
            .get("output")
            .expect("output tensor")
            .try_extract_tensor::<f32>()
            .expect("extract output");
        // 589 frames per 10 s window — the same number `frame_offset` is built on.
        assert_eq!(shape[1], 589, "unexpected frame count {shape:?}");
    }

    /// Embeds stretches whose speaker is known from the transcript and prints
    /// the distance matrix — the ground truth this module has no other way to
    /// get. Set `VOXMD_EMBED_AUDIO`, then edit `picks` to match that file.
    ///
    /// What it established for the shipped settings: two takes of one voice sit
    /// at 0.07–0.18 apart, two different voices at 0.55–0.67. That empty band is
    /// where [`CLUSTER_DIST`] has to land, and measuring it beat guessing —
    /// a value tuned against the old segmentation was above the band and merged
    /// every speaker into one.
    #[test]
    fn embedding_distance_matrix() {
        let Some(path) = std::env::var_os("VOXMD_EMBED_AUDIO") else {
            return;
        };
        let all = crate::audio::decode_file_to_mono_16k(&PathBuf::from(path), || false)
            .expect("decode audio");
        let pcm = to_i16(&all);
        let picks: [(&str, f32, f32); 6] = [
            ("A-1", 22.0, 34.0),
            ("A-2", 80.0, 92.0),
            ("A-3", 134.0, 146.0),
            ("B-1", 172.0, 184.0),
            ("B-2", 190.0, 202.0),
            ("B-3", 216.0, 228.0),
        ];
        onnx_runtime::init(&cache_dir()).expect("initialise ONNX Runtime");
        let mut extractor =
            pyannote_rs::EmbeddingExtractor::new(cached(EMB_FILE)).expect("embedder");
        let mut embeddings = Vec::new();
        for (name, from, to) in picks {
            let i0 = (from * SAMPLE_RATE as f32) as usize;
            let i1 = ((to * SAMPLE_RATE as f32) as usize).min(pcm.len());
            let mut v: Vec<f32> = extractor.compute(&pcm[i0..i1]).expect("embed").collect();
            l2_normalize(&mut v);
            embeddings.push((name, v));
        }
        let header: String = picks.iter().map(|p| format!("{:>9}", p.0)).collect();
        eprintln!("\n{:8}{header}", "");
        for (name, a) in &embeddings {
            let row: String = embeddings
                .iter()
                .map(|(_, b)| format!("{:9.3}", cosine_dist(a, b)))
                .collect();
            eprintln!("{name:8}{row}");
        }
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
