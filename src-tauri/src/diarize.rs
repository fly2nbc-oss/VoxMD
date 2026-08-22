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

/// Excerpt lengths handed to the embedder, longest first.
///
/// Every excerpt must be the *same* length: CAM++ embeddings move with duration
/// far more than with identity. Measured on one voice, a 4.8 s excerpt sat 0.96
/// from a 9.9 s excerpt of the same person — further apart than two different
/// people ever were, which let "how long was this" outrank "who is speaking".
/// At a fixed length the same voice stays within 0.20 and two voices never come
/// closer than 0.69.
///
/// Both entries were measured on real episodes; 6 s separates most cleanly
/// (same ≤0.20, other ≥0.71), 4 s is the fallback for material where nobody
/// speaks six uninterrupted seconds (same ≤0.32, other ≥0.69). Lengths between
/// them were *not* interchangeable — 3 s and 5 s collapsed every distance
/// toward zero — so do not interpolate: measure before adding one.
const EMBED_LENS: [f32; 2] = [6.0, 4.0];
/// Below this many usable anchors, drop to the next shorter excerpt length.
const MIN_ANCHORS: usize = 8;
/// Spans this close together are one stretch of speech with a breath in it, so
/// the pause is kept in the audio rather than cut out. Anything longer is where
/// the other voice sits, and the stretch ends there.
const MAX_EMBED_GAP_S: f32 = 0.5;
const MAX_ANCHORS: usize = 600;
/// Average-linkage cosine distance below which two clusters merge in auto mode.
///
/// Measured on the anchors the clusterer actually receives, not on hand-cut
/// excerpts: at a fixed excerpt length the same voice stays within 0.20 (6 s)
/// or 0.32 (4 s), and two voices never come closer than 0.69. 0.45 sits in that
/// empty band for both lengths.
///
/// The number is only meaningful together with how anchors are built. Tuned
/// against variable-length anchors it was hopeless at any value — that spread
/// ran to 0.99 within a single speaker. Fix the anchors before touching this.
const CLUSTER_DIST: f32 = 0.45;
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

/// One local speaker inside one window: the model's own output, so the audio
/// behind it is single-speaker by construction.
///
/// This is the unit that gets embedded and clustered. Building long runs first
/// and embedding those was the mistake: a run follows *track activity*, and the
/// track identity drifts whenever only one person speaks, so runs spanned turn
/// changes. Measured on a two-person podcast, roughly half the resulting
/// anchors sat between the two voices and the clustering grouped blends —
/// five clusters for two speakers, with no threshold able to fix it.
struct WindowSpeaker {
    /// Global frame indices this speaker holds, as `[from, to)` spans.
    spans: Vec<(usize, usize)>,
    frames: usize,
    embedding: Option<Vec<f32>>,
}

/// Collects the frames one local speaker holds in one window, as spans.
fn spans_for(
    activity: &[[bool; LOCAL_SPEAKERS]],
    base: usize,
    local: usize,
) -> (Vec<(usize, usize)>, usize) {
    let mut spans = Vec::new();
    let mut frames = 0usize;
    let mut start: Option<usize> = None;
    for f in 0..=activity.len() {
        let on = f < activity.len() && activity[f][local];
        match (start, on) {
            (None, true) => start = Some(f),
            (Some(from), false) => {
                spans.push((base + from, base + f));
                frames += f - from;
                start = None;
            }
            _ => {}
        }
    }
    (spans, frames)
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
    let mut units = window_speakers(&samples, SAMPLE_RATE, seg_path, abort)?;
    let duration_hist = duration_histogram(units.iter().map(|u| frames_to_secs(u.frames)));

    let runs: Vec<Option<(usize, usize)>> = units.iter().map(embed_span).collect();
    let embed_len = pick_embed_len(&runs, samples.len());

    for (unit, run) in units.iter_mut().zip(&runs) {
        if abort() {
            return Err("Cancelled.".to_string());
        }
        let Some((from, to)) = *run else { continue };
        let Some((a, b)) = embed_excerpt(from, to, samples.len(), embed_len) else {
            continue;
        };
        if let Ok(iter) = extractor.compute(&samples[a..b]) {
            let mut v: Vec<f32> = iter.collect();
            l2_normalize(&mut v);
            unit.embedding = Some(v);
        }
    }

    cluster_and_assign(&units, target_k, duration_hist, samples.len(), embed_len)
}

/// The stretch of a unit that gets embedded.
///
/// Not the concatenation of every span: splicing disjoint spans puts a step
/// discontinuity at each junction, and CAM++ reads those transients as voice.
/// Measured on a single-speaker intro, spliced windows landed 0.42–0.58 from
/// contiguous ones — as far apart as two different people, and enough to make
/// "how often was this spliced" outrank "who is speaking" in the clustering.
/// One real waveform, still one speaker by construction, separates cleanly.
fn pick_embed_len(runs: &[Option<(usize, usize)>], samples_len: usize) -> f32 {
    let last = *EMBED_LENS.last().expect("EMBED_LENS is not empty");
    EMBED_LENS
        .iter()
        .copied()
        .find(|&want| {
            runs.iter()
                .filter(|run| {
                    run.is_some_and(|(a, b)| embed_excerpt(a, b, samples_len, want).is_some())
                })
                .count()
                >= MIN_ANCHORS
        })
        .unwrap_or(last)
}

fn embed_span(unit: &WindowSpeaker) -> Option<(usize, usize)> {
    let longer = |c: &Option<(usize, usize)>| c.map_or(0, |(a, b)| b - a);
    let mut best: Option<(usize, usize)> = None;
    let mut run: Option<(usize, usize)> = None;
    for &(from, to) in &unit.spans {
        run = match run {
            Some((a, b)) if frames_to_secs(from.saturating_sub(b)) <= MAX_EMBED_GAP_S => {
                Some((a, to))
            }
            other => {
                best = std::cmp::max_by_key(best, other, longer);
                Some((from, to))
            }
        };
    }
    std::cmp::max_by_key(best, run, longer)
}

/// The exact slice handed to the embedder: `want_s` seconds centred in the run,
/// or nothing when the run is shorter. See [`EMBED_LENS`] for why it is fixed.
fn embed_excerpt(
    from: usize,
    to: usize,
    samples_len: usize,
    want_s: f32,
) -> Option<(usize, usize)> {
    let want = (want_s * SAMPLE_RATE as f32) as usize;
    let a = frame_offset(0, from);
    let b = frame_offset(0, to).min(samples_len);
    if b.saturating_sub(a) < want {
        return None;
    }
    let start = a + (b - a - want) / 2;
    Some((start, start + want))
}

fn frames_to_secs(frames: usize) -> f32 {
    (frames * FRAME_SIZE) as f32 / SAMPLE_RATE as f32
}

/// Clusters the per-window speakers and turns the result into speaker turns.
fn cluster_and_assign(
    units: &[WindowSpeaker],
    target_k: Option<usize>,
    duration_hist: [usize; 5],
    samples_len: usize,
    min_anchor_s: f32,
) -> Result<(Vec<SpeakerTurn>, DiarizeStats), String> {
    let n_with_embed = units.iter().filter(|u| u.embedding.is_some()).count();
    let mut anchor_idx: Vec<usize> = units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.embedding.is_some() && frames_to_secs(u.frames) >= min_anchor_s)
        .map(|(i, _)| i)
        .collect();

    if anchor_idx.is_empty() {
        // Fall back to any embedded unit so a short clip still labels.
        anchor_idx = units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.embedding.is_some())
            .map(|(i, _)| i)
            .collect();
    }
    if anchor_idx.is_empty() {
        return Err("No speaker turns found.".to_string());
    }
    anchor_idx.sort_by(|a, b| units[*b].frames.cmp(&units[*a].frames));
    anchor_idx.truncate(MAX_ANCHORS);

    let embeddings: Vec<Vec<f32>> = anchor_idx
        .iter()
        .map(|&i| units[i].embedding.clone().unwrap_or_default())
        .collect();
    let durations: Vec<f32> = anchor_idx
        .iter()
        .map(|&i| frames_to_secs(units[i].frames))
        .collect();

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

    // Every unit — anchors and the rest — takes the nearest centroid, then
    // writes its label into the frames it holds. Overlapping windows vote.
    let last_frame = units
        .iter()
        .flat_map(|u| u.spans.iter().map(|s| s.1))
        .max()
        .unwrap_or(0);
    let mut votes: Vec<Vec<f32>> = vec![vec![0.0; centroids.len()]; last_frame];
    for unit in units {
        let Some(emb) = unit.embedding.as_deref() else {
            continue;
        };
        let mut best = 0usize;
        let mut best_d = f32::MAX;
        for (i, c) in centroids.iter().enumerate() {
            let d = cosine_dist(emb, c);
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        // A confident unit counts for more where two windows disagree.
        let weight = (1.0 - best_d).max(0.05);
        for &(from, to) in &unit.spans {
            for frame in votes.iter_mut().take(to.min(last_frame)).skip(from) {
                frame[best] += weight;
            }
        }
    }

    let turns = turns_from_votes(&votes, samples_len);
    let turns = polish_turns(turns);

    let mut acc: HashMap<usize, f32> = HashMap::new();
    for &(start, end, id) in &turns {
        *acc.entry(id).or_insert(0.0) += (end - start).max(0.0);
    }
    let mut ids: Vec<usize> = acc.keys().copied().collect();
    ids.sort_unstable();
    let speech_per_cluster: Vec<f32> = ids.into_iter().map(|id| acc[&id]).collect();

    let stats = DiarizeStats {
        n_segments: units.len(),
        duration_hist,
        n_with_embed,
        n_anchors: anchor_idx.len(),
        merge_distances,
        n_clusters: speech_per_cluster.len(),
        speech_per_cluster,
        first_turns: turns.iter().copied().take(40).collect(),
        first_turns_all: turns.clone(),
    };
    Ok((turns, stats))
}

/// Frame-level winner per frame, collapsed into turns.
fn turns_from_votes(votes: &[Vec<f32>], samples_len: usize) -> Vec<SpeakerTurn> {
    let rate = SAMPLE_RATE as f64;
    let mut out: Vec<SpeakerTurn> = Vec::new();
    let mut run: Option<(usize, usize)> = None; // (label, from_frame)
    for f in 0..=votes.len() {
        let winner = if f < votes.len() {
            let total: f32 = votes[f].iter().sum();
            if total <= 0.0 {
                None
            } else {
                votes[f]
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(Ordering::Equal))
                    .map(|(i, _)| i)
            }
        } else {
            None
        };
        match (run, winner) {
            (None, Some(w)) => run = Some((w, f)),
            (Some((w, _)), Some(cur)) if cur == w => {}
            (Some((w, from)), cur) => {
                let a = frame_offset(0, from).min(samples_len);
                let b = frame_offset(0, f).clamp(a, samples_len);
                out.push(((a as f64 / rate) as f32, (b as f64 / rate) as f32, w));
                run = cur.map(|c| (c, f));
            }
            (None, None) => {}
        }
    }
    out
}

/// Upper edges of the reported duration buckets, in seconds.
const HIST_EDGES_S: [f32; 4] = [1.0, 4.0, 6.0, 10.0];

fn duration_histogram(durs: impl IntoIterator<Item = f32>) -> [usize; 5] {
    let mut hist = [0usize; 5];
    for d in durs {
        let slot = HIST_EDGES_S.iter().filter(|&&edge| d >= edge).count();
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

/// Runs pyannote segmentation over the whole file and returns one unit per
/// (window, local speaker).
///
/// segmentation-3.0 sees 10 s at a time and numbers the speakers it hears
/// *within that window*. Those indices mean nothing across windows — but inside
/// one window they are exactly what is wanted: a single speaker's frames,
/// separated by the model itself. Each such group is embedded and clustered on
/// its own, and the global identity falls out of the clustering.
///
/// Windows overlap by 50 %, so a speaker crossing a boundary contributes a unit
/// on each side and the clustering ties them together. Nothing is stitched, and
/// nothing spans a turn change.
///
/// Upstream bugs fixed along the way (pyannote-rs 0.3.4 `get_segments`):
/// 1. i16 samples must be scaled to [-1, 1] or every frame reads as non-speech
///    (thewh1teagle/pyannote-rs#28).
/// 2. Speech lasting until EOF is flushed rather than dropped.
/// 3. The original `from_fn` stopped when a window produced no *closed*
///    segment — typical for a long opening utterance — losing the rest.
/// 4. The frame counter is anchored per window (see [`frame_offset`]).
fn window_speakers(
    samples: &[i16],
    sample_rate: u32,
    model_path: &Path,
    abort: &impl Fn() -> bool,
) -> Result<Vec<WindowSpeaker>, String> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let mut session = seg_session(model_path)?;
    let window_size = (sample_rate as usize).saturating_mul(10);
    if window_size == 0 {
        return Err("Invalid sample rate for diarization.".to_string());
    }

    let mut out = Vec::new();
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

        for local in 0..LOCAL_SPEAKERS {
            let (spans, frames) = spans_for(&activity, base, local);
            if frames > 0 {
                out.push(WindowSpeaker {
                    spans,
                    frames,
                    embedding: None,
                });
            }
        }

        if end == samples.len() {
            break;
        }
        window_start += hop_frames * FRAME_SIZE;
        base += hop_frames;
    }
    Ok(out)
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

    /// The unit that gets embedded must be one speaker's frames and nothing
    /// else — that is the whole point of clustering per window speaker.
    #[test]
    fn spans_cover_exactly_one_local_speaker() {
        let mut activity = vec![[false; LOCAL_SPEAKERS]; 10];
        for frame in activity.iter_mut().take(4).skip(1) {
            frame[0] = true;
        }
        for frame in activity.iter_mut().take(9).skip(6) {
            frame[0] = true;
        }
        for frame in activity.iter_mut().take(8).skip(2) {
            frame[1] = true;
        }

        let (spans, frames) = spans_for(&activity, 100, 0);
        assert_eq!(spans, vec![(101, 104), (106, 109)]);
        assert_eq!(frames, 6);

        let (spans, frames) = spans_for(&activity, 100, 1);
        assert_eq!(spans, vec![(102, 108)]);
        assert_eq!(frames, 6);

        // A speaker the window never heard produces no unit at all.
        let (spans, frames) = spans_for(&activity, 100, 2);
        assert!(spans.is_empty());
        assert_eq!(frames, 0);
    }

    #[test]
    fn votes_collapse_into_turns() {
        // Frames 0..3 speaker 0, 3..5 speaker 1, 5..6 silent, 6..8 speaker 0.
        let mut votes = vec![vec![0.0f32; 2]; 8];
        for v in votes.iter_mut().take(3) {
            v[0] = 1.0;
        }
        for v in votes.iter_mut().take(5).skip(3) {
            v[1] = 1.0;
        }
        for v in votes.iter_mut().take(8).skip(6) {
            v[0] = 1.0;
        }
        let turns = turns_from_votes(&votes, usize::MAX);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].2, 0);
        assert_eq!(turns[1].2, 1);
        assert_eq!(turns[2].2, 0);
        // The silent frame ends a turn rather than joining either side.
        assert!(turns[1].1 < turns[2].0);
    }

    #[test]
    fn overlapping_windows_vote_and_the_confident_one_wins() {
        let mut votes = vec![vec![0.0f32; 2]; 4];
        for v in votes.iter_mut() {
            v[0] = 0.3; // a hesitant window
            v[1] = 0.9; // a confident one covering the same frames
        }
        let turns = turns_from_votes(&votes, usize::MAX);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].2, 1);
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
        // Same spec as the scorer: NAME:from-to, comma separated.
        let spec = std::env::var("VOXMD_EMBED_PICKS").unwrap_or_else(|_| {
            "A-1:22-34,A-2:80-92,A-3:134-146,B-1:172-184,B-2:190-202,B-3:216-228".to_string()
        });
        let picks: Vec<(String, f32, f32)> = spec
            .split(',')
            .filter_map(|part| {
                let (name, span) = part.trim().split_once(':')?;
                let (from, to) = span.split_once('-')?;
                Some((name.to_string(), from.parse().ok()?, to.parse().ok()?))
            })
            .collect();
        onnx_runtime::init(&cache_dir()).expect("initialise ONNX Runtime");
        let mut extractor =
            pyannote_rs::EmbeddingExtractor::new(cached(EMB_FILE)).expect("embedder");
        let mut embeddings = Vec::new();
        for (name, from, to) in &picks {
            let i0 = (*from * SAMPLE_RATE as f32) as usize;
            let i1 = ((*to * SAMPLE_RATE as f32) as usize).min(pcm.len());
            let mut v: Vec<f32> = extractor.compute(&pcm[i0..i1]).expect("embed").collect();
            l2_normalize(&mut v);
            embeddings.push((name.clone(), v));
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

    /// A pause inside one person's sentence stays in the audio; the other
    /// voice ends the stretch. Splicing across it instead put a step
    /// discontinuity in the waveform that CAM++ read as a different speaker.
    #[test]
    fn embed_span_keeps_a_breath_but_ends_at_the_other_voice() {
        let unit = |spans: Vec<(usize, usize)>| WindowSpeaker {
            spans,
            frames: 0,
            embedding: None,
        };
        // 20 frames is 0.34 s — a breath, so both spans and the gap are one run.
        assert_eq!(
            embed_span(&unit(vec![(0, 100), (120, 200)])),
            Some((0, 200))
        );
        // 200 frames is 3.4 s — the other voice. The longer side wins.
        assert_eq!(
            embed_span(&unit(vec![(0, 100), (300, 480)])),
            Some((300, 480))
        );
        assert_eq!(embed_span(&unit(vec![])), None);
    }

    /// Every excerpt is the same length or there is none: a shorter one embeds
    /// nowhere near the same voice's longer ones.
    #[test]
    fn embed_excerpt_is_one_fixed_length_or_nothing() {
        let plenty = usize::MAX / 2;
        let want = EMBED_LENS[0];
        let (a, b) = embed_excerpt(0, 600, plenty, want).expect("600 frames is over 10 s");
        assert_eq!(b - a, (want * SAMPLE_RATE as f32) as usize);
        // Centred in the run.
        assert_eq!(a - frame_offset(0, 0), frame_offset(0, 600) - b);
        assert_eq!(embed_excerpt(0, 100, plenty, want), None);
    }

    #[test]
    fn pick_embed_len_falls_back_when_nobody_speaks_long_enough() {
        let plenty = usize::MAX / 2;
        let runs = |to: usize| vec![Some((0usize, to)); MIN_ANCHORS];
        assert_eq!(pick_embed_len(&runs(600), plenty), EMBED_LENS[0]);
        // 300 frames is 5.1 s: short of the 6 s excerpt, long enough for 4 s.
        assert_eq!(pick_embed_len(&runs(300), plenty), EMBED_LENS[1]);
        // One anchor short of the quorum also drops down.
        assert_eq!(
            pick_embed_len(&[Some((0, 600)); MIN_ANCHORS - 1], plenty),
            EMBED_LENS[1]
        );
        assert_eq!(pick_embed_len(&[], plenty), EMBED_LENS[1]);
    }

    /// Embeds the pipeline's OWN anchors and scores them against known voices.
    ///
    /// [`embedding_distance_matrix`] only ever proved that the embedder can tell
    /// two voices apart on hand-cut excerpts. It says nothing about the units
    /// clustering actually receives, and that is where every failure was: every
    /// anchor falling entirely inside a hand-verified stretch gets that
    /// stretch's speaker, and the within/between distances are printed.
    ///
    /// What it found, on a single speaker's uninterrupted intro: anchors of one
    /// person spread to 0.99 — wider than two people ever were — because the
    /// audio was spliced across pauses and the excerpts differed in length.
    /// With [`EMBED_LENS`] in place the same run reads: same voice ≤0.20, other
    /// voice ≥0.71.
    ///
    /// `VOXMD_ANCHOR_AUDIO` picks the file, `VOXMD_ANCHOR_REFS` the verified
    /// stretches (`A:0-50,B:182-232`, seconds), `VOXMD_ANCHOR_FIXED` overrides
    /// the excerpt length, `VOXMD_ANCHOR_DUMP` adds the full distance matrix.
    #[test]
    fn anchor_purity() {
        let Some(path) = std::env::var_os("VOXMD_ANCHOR_AUDIO") else {
            return;
        };
        let refs: Vec<(String, f32, f32)> = std::env::var("VOXMD_ANCHOR_REFS")
            .expect("VOXMD_ANCHOR_REFS")
            .split(',')
            .filter_map(|part| {
                let (name, span) = part.trim().split_once(':')?;
                let (from, to) = span.split_once('-')?;
                Some((name.to_string(), from.parse().ok()?, to.parse().ok()?))
            })
            .collect();

        let all = crate::audio::decode_file_to_mono_16k(&PathBuf::from(path), || false)
            .expect("decode audio");
        let samples = to_i16(&all);
        onnx_runtime::init(&cache_dir()).expect("initialise ONNX Runtime");
        let mut units =
            window_speakers(&samples, SAMPLE_RATE, &cached(SEG_FILE), &|| false).expect("segment");
        let mut extractor =
            pyannote_rs::EmbeddingExtractor::new(cached(EMB_FILE)).expect("embedder");

        let want = std::env::var("VOXMD_ANCHOR_FIXED")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(EMBED_LENS[0]);
        let mut tagged: Vec<(String, f32, Vec<f32>)> = Vec::new();
        for unit in units.iter_mut() {
            let Some(run) = embed_span(unit) else {
                continue;
            };
            let lo = frame_offset(0, unit.spans.first().map(|s| s.0).unwrap_or(0)) as f32
                / SAMPLE_RATE as f32;
            let hi = frame_offset(0, unit.spans.last().map(|s| s.1).unwrap_or(0)) as f32
                / SAMPLE_RATE as f32;
            let Some((name, _, _)) = refs.iter().find(|(_, a, b)| lo >= *a && hi <= *b) else {
                continue;
            };
            let (from, to) = run;
            let Some((a, b)) = embed_excerpt(from, to, samples.len(), want) else {
                continue;
            };
            let audio = &samples[a..b];
            if std::env::var_os("VOXMD_ANCHOR_DUMP").is_some() {
                eprintln!(
                    "  spans={:3} lo={:7.2}s hi={:7.2}s aktiv={:5.2}s audio={:5.2}s",
                    unit.spans.len(),
                    lo,
                    hi,
                    frames_to_secs(unit.frames),
                    audio.len() as f32 / SAMPLE_RATE as f32
                );
            }
            if let Ok(iter) = extractor.compute(audio) {
                let mut v: Vec<f32> = iter.collect();
                l2_normalize(&mut v);
                tagged.push((name.clone(), frames_to_secs(unit.frames), v));
            }
        }

        eprintln!(
            "
Anker mit bekannter Stimme: {}",
            tagged.len()
        );
        let mut within: Vec<f32> = Vec::new();
        let mut between: Vec<f32> = Vec::new();
        for (i, (na, _, a)) in tagged.iter().enumerate() {
            for (nb, _, b) in tagged.iter().skip(i + 1) {
                let d = cosine_dist(a, b);
                if na == nb {
                    within.push(d)
                } else {
                    between.push(d)
                }
            }
        }
        let stat = |v: &mut Vec<f32>| {
            if v.is_empty() {
                return "-".to_string();
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            format!(
                "n={} min={:.3} med={:.3} p90={:.3} max={:.3}",
                v.len(),
                v[0],
                v[v.len() / 2],
                v[v.len() * 9 / 10],
                v[v.len() - 1]
            )
        };
        eprintln!("gleiche Stimme:  {}", stat(&mut within));
        eprintln!("andere Stimme:   {}", stat(&mut between));
        if std::env::var_os("VOXMD_ANCHOR_DUMP").is_none() {
            return;
        }
        let header: String = (0..tagged.len()).map(|i| format!("{i:>7}")).collect();
        eprintln!("\n{:6}{header}", "");
        for (i, (na, sa, a)) in tagged.iter().enumerate() {
            let row: String = tagged
                .iter()
                .map(|(_, _, b)| format!("{:7.3}", cosine_dist(a, b)))
                .collect();
            eprintln!("{i:2} {na:>2}{sa:4.1}{row}");
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

#[cfg(test)]
mod quality {
    use super::*;
    use crate::llm::TranscriptLine;

    /// Reference labels per line, from the embedder alone: each line's audio is
    /// embedded and matched to hand-verified voice samples. No clustering, so
    /// this measures the pipeline, not the embedder.
    fn reference(
        pcm: &[i16],
        lines: &[TranscriptLine],
        refs: &[(&str, f32, f32)],
    ) -> Vec<Option<usize>> {
        onnx_runtime::init(&cache_dir()).expect("ort");
        let mut ex = pyannote_rs::EmbeddingExtractor::new(cached(EMB_FILE)).expect("embedder");
        let embed =
            |ex: &mut pyannote_rs::EmbeddingExtractor, a: f32, b: f32| -> Option<Vec<f32>> {
                let i0 = (a * SAMPLE_RATE as f32) as usize;
                let i1 = ((b * SAMPLE_RATE as f32) as usize).min(pcm.len());
                // Six seconds, not the 1.5 s an anchor needs. A reference has
                // to be *right*, not plentiful: at 1.5 s the per-line
                // embeddings are noise — one speaker's three-minute monologue
                // came back with labels alternating line by line, and scoring
                // against that measures nothing. Fewer trustworthy lines beat
                // many uncertain ones, the same lesson as the anchors.
                if i1 <= i0 || (i1 - i0) < SAMPLE_RATE as usize * 6 {
                    return None;
                }
                let mut v: Vec<f32> = ex.compute(&pcm[i0..i1]).ok()?.collect();
                l2_normalize(&mut v);
                Some(v)
            };
        // One centroid per named voice.
        let mut names: Vec<&str> = Vec::new();
        let mut sums: Vec<Vec<f32>> = Vec::new();
        for (name, a, b) in refs {
            let v = embed(&mut ex, *a, *b).expect("reference sample too short");
            match names.iter().position(|n| n == name) {
                Some(i) => {
                    for (x, y) in sums[i].iter_mut().zip(v.iter()) {
                        *x += y;
                    }
                }
                None => {
                    names.push(name);
                    sums.push(v);
                }
            }
        }
        for v in sums.iter_mut() {
            l2_normalize(v);
        }

        lines
            .iter()
            .map(|l| {
                let v = embed(&mut ex, l.start, l.end)?;
                let mut d: Vec<(usize, f32)> = sums
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (i, cosine_dist(&v, c)))
                    .collect();
                d.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
                // Relative, not absolute: a line counts as reference only when
                // it is clearly nearer one voice than the other. Short lines
                // embed noisily, and an absolute cutoff excluded 83% of them.
                // Wide enough to score two thirds of the lines while the
                // reference stays trustworthy; measured, not guessed.
                let margin = 0.05f32;
                if d.len() < 2 || d[1].1 - d[0].1 < margin {
                    None
                } else {
                    Some(d[0].0)
                }
            })
            .collect()
    }

    /// Best mapping from pipeline labels onto reference voices, then accuracy.
    fn score(pred: &[Option<usize>], gold: &[Option<usize>]) -> (usize, usize) {
        let mut map: HashMap<usize, HashMap<usize, usize>> = HashMap::new();
        for (p, g) in pred.iter().zip(gold.iter()) {
            if let (Some(p), Some(g)) = (p, g) {
                *map.entry(*p).or_default().entry(*g).or_insert(0) += 1;
            }
        }
        let best: HashMap<usize, usize> = map
            .iter()
            .map(|(p, counts)| (*p, *counts.iter().max_by_key(|(_, n)| **n).unwrap().0))
            .collect();
        let mut hit = 0;
        let mut total = 0;
        for (p, g) in pred.iter().zip(gold.iter()) {
            if let Some(g) = g {
                total += 1;
                if p.and_then(|p| best.get(&p)) == Some(g) {
                    hit += 1;
                }
            }
        }
        (hit, total)
    }

    /// Scores the pipeline against a reference built from the embedder alone.
    ///
    /// The only ground truth available here. Hand-verified voice samples become
    /// centroids, every transcript line is embedded and matched to the nearer
    /// one, and lines without a clear winner are excluded rather than guessed
    /// at. No clustering is involved, so this measures everything downstream of
    /// the embedder — which is where the faults were.
    ///
    /// Usage: dump a transcript to TSV (`start\tend\ttext`), point
    /// `VOXMD_SCORE_AUDIO` and `VOXMD_SCORE_LINES` at the pair, and edit `refs`
    /// to stretches whose speaker you have confirmed by reading the transcript.
    /// It reports the majority-class rate too, so a number that a constant
    /// predictor could reach is visible as such.
    ///
    /// State when this was written, on a 12-minute two-person podcast:
    /// 84.1% over 107 scorable lines, against a 60.7% majority baseline, with
    /// the pipeline emitting five clusters for two speakers. The cluster count
    /// is the open problem — see the note in CLAUDE.md.
    /// Writes a transcript to TSV so the scorer can be re-run without paying
    /// for transcription each time. `VOXMD_DUMP_AUDIO`, `VOXMD_DUMP_OUT`.
    #[test]
    fn dump_transcript() {
        let Some(path) = std::env::var_os("VOXMD_DUMP_AUDIO") else {
            return;
        };
        let out = std::env::var("VOXMD_DUMP_OUT").expect("VOXMD_DUMP_OUT");
        let secs: usize = std::env::var("VOXMD_DUMP_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(720);
        let all = crate::audio::decode_file_to_mono_16k(&PathBuf::from(path), || false)
            .expect("decode audio");
        let n = (secs * SAMPLE_RATE as usize).min(all.len());
        let model = crate::model_download::cache_dir().join("ggml-large-v3-turbo.bin");
        assert!(model.is_file(), "missing {}", model.display());
        let ctx = whisper_rs::WhisperContext::new_with_params(
            model.to_str().expect("model path"),
            whisper_rs::WhisperContextParameters {
                use_gpu: true,
                ..Default::default()
            },
        )
        .expect("whisper init");
        let mut state = ctx.create_state().expect("state");
        let mut params =
            whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("de"));
        params.set_n_threads(8);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        state.full(params, &all[..n]).expect("transcribe");
        let lines = crate::llm::lines_from_state(&state).expect("lines");
        let mut text = String::new();
        for l in &lines {
            text.push_str(&format!("{:.2}\t{:.2}\t{}\n", l.start, l.end, l.text));
        }
        std::fs::write(&out, text).expect("write tsv");
        eprintln!("{} Zeilen -> {out}", lines.len());
    }

    #[test]
    fn accuracy_against_reference() {
        let Some(path) = std::env::var_os("VOXMD_SCORE_AUDIO") else {
            return;
        };
        let tsv = std::env::var("VOXMD_SCORE_LINES").expect("VOXMD_SCORE_LINES");
        let secs: usize = std::env::var("VOXMD_SCORE_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(720);

        let all =
            crate::audio::decode_file_to_mono_16k(&PathBuf::from(path), || false).expect("decode");
        let n = (secs * SAMPLE_RATE as usize).min(all.len());
        let samples = &all[..n];
        let pcm = to_i16(samples);

        let lines: Vec<TranscriptLine> = std::fs::read_to_string(&tsv)
            .expect("lines")
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\t');
                Some(TranscriptLine {
                    start: f.next()?.parse().ok()?,
                    end: f.next()?.parse().ok()?,
                    text: f.next()?.to_string(),
                })
            })
            .filter(|l| l.end <= secs as f32)
            .collect();

        // Voice samples, hand-verified by reading the transcript. Per file, so
        // they come from the environment rather than being wired to one podcast:
        //   VOXMD_SCORE_REFS="A:22-34,A:80-92,B:172-184,B:190-202"
        let spec = std::env::var("VOXMD_SCORE_REFS")
            .unwrap_or_else(|_| "A:22-34,A:80-92,A:134-146,B:172-184,B:190-202".to_string());
        let owned: Vec<(String, f32, f32)> = spec
            .split(',')
            .filter_map(|part| {
                let (name, span) = part.trim().split_once(':')?;
                let (from, to) = span.split_once('-')?;
                Some((name.to_string(), from.parse().ok()?, to.parse().ok()?))
            })
            .collect();
        assert!(owned.len() >= 2, "need at least two voice samples: {spec}");
        let refs: Vec<(&str, f32, f32)> =
            owned.iter().map(|(n, a, b)| (n.as_str(), *a, *b)).collect();
        let gold = reference(&pcm, &lines, &refs);

        let forced: u8 = std::env::var("VOXMD_SCORE_SPEAKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let (turns, _) = run_diarize(
            samples,
            &cached(SEG_FILE),
            &cached(EMB_FILE),
            forced,
            &|| false,
        )
        .expect("diarize");
        let mut pred = speakers_for_lines(&lines, &turns);
        smooth_speaker_outliers(&lines, &mut pred);

        // A metric that a constant predictor can win measures nothing. Report the
        // majority-class rate alongside, and the confusion, so that is visible.
        let mut counts: HashMap<usize, usize> = HashMap::new();
        for g in gold.iter().flatten() {
            *counts.entry(*g).or_insert(0) += 1;
        }
        let scored: usize = counts.values().sum();
        let majority = counts.values().copied().max().unwrap_or(0);
        let mut dist: Vec<String> = counts
            .iter()
            .map(|(k, v)| format!("Stimme {k}: {v}"))
            .collect();
        dist.sort();
        let npred = pred
            .iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len();
        eprintln!(
            "REFERENZ  {}  |  Mehrheitsklasse {:.1}%  |  Pipeline-Cluster: {npred}",
            dist.join(", "),
            100.0 * majority as f32 / scored.max(1) as f32
        );

        if let Some(dump) = std::env::var_os("VOXMD_SCORE_DUMP") {
            let n = dump
                .to_str()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(30);
            for (i, line) in lines.iter().enumerate().take(n) {
                let g = gold[i].map(|g| g.to_string()).unwrap_or_else(|| "-".into());
                let p = pred[i].map(|p| p.to_string()).unwrap_or_else(|| "-".into());
                eprintln!(
                    "{:7.1}  ref={g}  pred={p}  {}",
                    line.start,
                    line.text.chars().take(70).collect::<String>()
                );
            }
        }

        let (hit, total) = score(&pred, &gold);
        eprintln!(
            "GENAUIGKEIT {:.1}%  ({hit}/{total} bewertbare Zeilen, {} Zeilen ohne Referenz)",
            100.0 * hit as f32 / total as f32,
            gold.iter().filter(|g| g.is_none()).count()
        );
    }
}
