use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::decode_file_to_mono_16k;
use crate::config::AppConfig;
use crate::llm::{self, make_client};
use crate::meta::{
    self, get_audio_metadata, get_audio_path_for_episode, get_md_path, get_md_path_for_episode,
};
use crate::model_download;
use crate::podcast::{self, QueueItem};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgressPayload {
    /// Queue item id (local path or episode URL) — the frontend list key.
    pub path: String,
    pub display_name: String,
    /// queued, download, whisper, llm, done, skipped, error
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whisper_pct: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_pct: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall: Option<OverallProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverallProgress {
    pub completed: usize,
    pub total: usize,
    pub pct: f32,
}

static PROCESSING: AtomicBool = AtomicBool::new(false);
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn is_processing() -> bool {
    PROCESSING.load(Ordering::SeqCst)
}

/// Claims the processing slot and clears any stale cancel flag.
///
/// Called synchronously from `start_transcription` *before* it returns, so the
/// frontend can never enable its Cancel button while `PROCESSING` is still false
/// (which used to make `request_cancel` a no-op) and never has its cancel erased
/// by a later reset.
pub fn begin_batch() -> Result<(), String> {
    PROCESSING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "Processing is already running.".to_string())?;
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    Ok(())
}

/// Releases the processing slot on drop, so no early return (including a panic
/// unwinding out of `run_batch`) can leave the app permanently wedged.
struct ProcessingGuard;

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        CANCEL_REQUESTED.store(false, Ordering::SeqCst);
        PROCESSING.store(false, Ordering::SeqCst);
    }
}

pub fn request_cancel() {
    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
}

pub(crate) fn cancel_requested() -> bool {
    CANCEL_REQUESTED.load(Ordering::SeqCst)
}

fn emit_job(app: &AppHandle, p: JobProgressPayload) {
    let _ = app.emit("job_progress", &p);
}

fn payload(id: &str, display_name: &str, stage: &str) -> JobProgressPayload {
    JobProgressPayload {
        path: id.to_string(),
        display_name: display_name.to_string(),
        stage: stage.to_string(),
        whisper_pct: None,
        download_pct: None,
        overall: None,
        message: None,
    }
}

fn whisper_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).max(1))
        .unwrap_or(4)
}

type ProgressCb = Box<dyn FnMut(i32)>;
type AbortCb = Box<dyn FnMut() -> bool>;

fn transcribe_one(
    ctx: &WhisperContext,
    audio_path: &Path,
    cfg: &AppConfig,
    on_progress: impl FnMut(i32) + 'static,
) -> Result<String, String> {
    let samples = decode_file_to_mono_16k(audio_path)?;

    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let lang = cfg.language.trim();
    let lang = if lang.is_empty() { "auto" } else { lang };
    params.set_language(Some(lang));
    params.set_n_threads(whisper_threads() as i32);
    // Inference is by far the longest operation in the app. Without the abort
    // callback a cancel could not take effect until the whole file was done.
    // Both need an explicit turbofish: the `O: Into<Option<F>>` bound leaves `F`
    // ambiguous for the trait solver.
    params
        .set_progress_callback_safe::<Option<ProgressCb>, ProgressCb>(Some(Box::new(on_progress)));
    params.set_abort_callback_safe::<Option<AbortCb>, AbortCb>(Some(Box::new(cancel_requested)));
    state
        .full(params, &samples)
        .map_err(|e| format!("Whisper inference: {e}"))?;

    if cancel_requested() {
        return Err("Cancelled.".to_string());
    }

    llm::segments_to_raw_text(&state)
}

/// Queue item with resolved output target and display metadata.
#[derive(Clone)]
struct WorkItem {
    item: QueueItem,
    md_path: PathBuf,
    /// Local audio file used for Whisper (and optional deletion after success).
    local_audio: PathBuf,
    title: String,
    /// Tag year for local files; podcast items carry the date in `episode`.
    year: Option<String>,
}

fn prepare_work_item(item: &QueueItem) -> Result<WorkItem, String> {
    if item.is_podcast() {
        let Some(ep) = item.episode.clone() else {
            return Err("Podcast entry without episode metadata.".to_string());
        };
        let out_dir = PathBuf::from(ep.output_dir.trim());
        if !out_dir.is_dir() {
            return Err(format!(
                "Podcast output folder not found: {}",
                out_dir.display()
            ));
        }
        // Episodes written before the stem carried the full date keep their old
        // name, so upgrading does not re-transcribe an existing library.
        let legacy = meta::legacy_md_path_for_episode(&out_dir, &ep.title, &ep.date);
        let md_path = if legacy.exists() {
            legacy
        } else {
            get_md_path_for_episode(&out_dir, &ep.title, &ep.date)
        };
        let local_audio = get_audio_path_for_episode(
            &out_dir,
            &ep.title,
            &ep.date,
            &podcast::url_extension(&item.source),
        );
        Ok(WorkItem {
            item: item.clone(),
            md_path,
            local_audio,
            title: ep.title,
            year: None,
        })
    } else {
        let path = PathBuf::from(&item.source);
        let (title, year) = get_audio_metadata(&path);
        let md_path = get_md_path(&path, &title, &year);
        Ok(WorkItem {
            item: item.clone(),
            md_path,
            local_audio: path,
            title,
            year,
        })
    }
}

/// Metadata block under the H1 (toggled by `include_meta`).
fn meta_block(work: &WorkItem) -> String {
    let mut lines = Vec::new();
    if let Some(ep) = &work.item.episode {
        lines.push(format!("- Podcast: {}", ep.feed_title));
        lines.push(format!("- Episode: {}", ep.title));
        if let Some(d) = &ep.date {
            lines.push(format!("- Published: {d}"));
        }
        if let Some(l) = &ep.link {
            lines.push(format!("- Link: {l}"));
        }
    } else {
        let file_name = Path::new(&work.item.source)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        lines.push(format!("- Source: {file_name}"));
        if let Some(y) = &work.year {
            lines.push(format!("- Year: {y}"));
        }
    }
    lines.join("\n")
}

/// Orientation block for the summary prompt.
fn summary_context(work: &WorkItem) -> String {
    if let Some(ep) = &work.item.episode {
        let mut s = format!("Podcast: {}\nEpisode: {}", ep.feed_title, ep.title);
        if let Some(d) = &ep.date {
            s.push_str(&format!("\nPublished: {d}"));
        }
        s
    } else {
        format!("Title: {}", work.title)
    }
}

pub struct TranscribedJob {
    work: WorkItem,
    raw_text: String,
}

async fn llm_stage(
    app: AppHandle,
    cfg: AppConfig,
    job: TranscribedJob,
    done_counter: Arc<AtomicUsize>,
    total: usize,
) {
    let id = job.work.item.id.clone();
    let display_name = job.work.item.display_name.clone();

    if cancel_requested() {
        let mut p = payload(&id, &display_name, "skipped");
        p.message = Some("Cancelled.".to_string());
        emit_job(&app, p);
        return;
    }

    let transcript = job.raw_text;

    // Skipped silently when no API key is configured (see AppConfig::summary_enabled).
    let summary = if cfg.summary_enabled() {
        let mut p = payload(&id, &display_name, "llm");
        p.message = Some("Summary…".to_string());
        emit_job(&app, p);

        let client = make_client(&cfg);
        match llm::generate_summary(&client, &cfg, &summary_context(&job.work), &transcript).await {
            Ok(s) => s.trim().to_string(),
            Err(e) => {
                emit_error(
                    &app,
                    &id,
                    &display_name,
                    format!("Summary: {e}"),
                    &done_counter,
                    total,
                );
                return;
            }
        }
    } else {
        String::new()
    };

    // Refuse to write a file that would carry no content. Without this an empty
    // completion plus `include_transcript = false` produced a bare "# Title" —
    // and the source audio was then deleted below.
    if summary.is_empty() && !cfg.include_transcript {
        emit_error(
            &app,
            &id,
            &display_name,
            "Summary was empty and the transcript is disabled — nothing to write. \
             Source file kept."
                .to_string(),
            &done_counter,
            total,
        );
        return;
    }

    let mut content = format!("# {}\n", job.work.title);
    if cfg.include_meta {
        content.push('\n');
        content.push_str(&meta_block(&job.work));
        content.push('\n');
    }
    if !summary.is_empty() {
        content.push('\n');
        content.push_str(&summary);
        content.push('\n');
    }
    if cfg.include_transcript {
        content.push_str("\n## Transcript\n\n");
        content.push_str(&transcript);
        content.push('\n');
    }

    let md_path = &job.work.md_path;
    // Blocking write on an async task: transcripts reach a few MB, so hand it to
    // the blocking pool rather than stalling a runtime worker.
    let write_res = {
        let md_path = md_path.clone();
        tokio::task::spawn_blocking(move || std::fs::write(md_path, content))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
    };
    if let Err(e) = write_res {
        emit_error(
            &app,
            &id,
            &display_name,
            format!("Save failed: {e}"),
            &done_counter,
            total,
        );
        return;
    }

    // Optional: delete audio only. Markdown was just written and is never removed.
    let deletion_note = if cfg.delete_source_after_success {
        let audio = &job.work.local_audio;
        if audio == md_path {
            String::new()
        } else {
            match std::fs::remove_file(audio) {
                Ok(()) => String::new(),
                Err(e) => format!(" (audio deletion failed: {e})"),
            }
        }
    } else {
        String::new()
    };

    let c = done_counter.fetch_add(1, Ordering::SeqCst) + 1;
    let mut p = payload(&id, &display_name, "done");
    p.overall = Some(progress(c, total));
    p.message = Some(format!("Saved: {}{}", md_path.display(), deletion_note));
    emit_job(&app, p);
}

fn progress(completed: usize, total: usize) -> OverallProgress {
    OverallProgress {
        completed,
        total,
        pct: if total > 0 {
            (completed as f32 / total as f32) * 100.0
        } else {
            100.0
        },
    }
}

fn overall_snapshot(done: &Arc<AtomicUsize>, total: usize) -> OverallProgress {
    progress(done.load(Ordering::SeqCst), total)
}

/// Terminal failure for one item. Counts it as settled so the overall bar still
/// reaches 100% when some items fail — previously only the success path counted.
fn emit_error(
    app: &AppHandle,
    id: &str,
    display_name: &str,
    message: String,
    done: &Arc<AtomicUsize>,
    total: usize,
) {
    let c = done.fetch_add(1, Ordering::SeqCst) + 1;
    let mut p = payload(id, display_name, "error");
    p.overall = Some(progress(c, total));
    p.message = Some(message);
    emit_job(app, p);
}

fn emit_skipped_tail(
    app: &AppHandle,
    work: &[WorkItem],
    from: usize,
    done: &Arc<AtomicUsize>,
    total: usize,
) {
    for wi in work.iter().skip(from) {
        let mut p = payload(&wi.item.id, &wi.item.display_name, "skipped");
        p.overall = Some(overall_snapshot(done, total));
        p.message = Some("Cancelled.".to_string());
        emit_job(app, p);
    }
}

/// Pipeline: Whisper (file N+1) and LLM (file N) run in parallel,
/// but never more than one of each. Channel capacity=1 enforces this.
///
/// The caller must already hold the processing slot via [`begin_batch`].
/// Exactly one `batch_complete` is emitted on every exit path — the frontend
/// clears its `processing` flag there and would otherwise hang forever.
pub async fn run_batch(app: AppHandle, items: Vec<QueueItem>, cfg: AppConfig) {
    let _guard = ProcessingGuard;

    let (total, result) = run_batch_inner(&app, items, cfg).await;
    let cancelled = cancel_requested();

    let mut done = serde_json::json!({ "total": total, "cancelled": cancelled });
    if let Err(e) = &result {
        done["error"] = serde_json::Value::String(e.clone());
    }
    let _ = app.emit("batch_complete", &done);
}

/// Returns the number of items that entered the pipeline plus the batch outcome.
async fn run_batch_inner(
    app: &AppHandle,
    items: Vec<QueueItem>,
    cfg: AppConfig,
) -> (usize, Result<(), String>) {
    if let Err(e) = cfg.validate_for_run() {
        return (0, Err(e));
    }

    // Resolve Whisper model (download from HuggingFace if name given and not cached).
    let model_path = {
        let app_dl = app.clone();
        let model_name = cfg.whisper_model.clone();
        let _ = app_dl.emit(
            "model_download_progress",
            serde_json::json!({ "stage": "resolving", "model": model_name }),
        );
        // Throttled to whole percent: the raw callback fires per chunk, which for a
        // 3 GB model would flood the webview with hundreds of thousands of events.
        let last_pct = AtomicI32::new(-1);
        match model_download::resolve_model(&model_name.clone(), move |dl, total| {
            let pct = (dl * 100).checked_div(total).unwrap_or(0) as i32;
            if last_pct.swap(pct, Ordering::Relaxed) == pct {
                return;
            }
            let _ = app_dl.emit(
                "model_download_progress",
                serde_json::json!({
                    "stage": "downloading",
                    "model": model_name,
                    "downloaded": dl,
                    "total": total,
                    "pct": pct,
                }),
            );
        })
        .await
        {
            Ok(p) => p,
            Err(e) => return (0, Err(e)),
        }
    };
    let _ = app.emit(
        "model_download_progress",
        serde_json::json!({ "stage": "ready", "path": model_path.display().to_string() }),
    );

    let mut work: Vec<WorkItem> = Vec::new();
    for item in items {
        let wi = match prepare_work_item(&item) {
            Ok(w) => w,
            Err(e) => {
                let mut p = payload(&item.id, &item.display_name, "error");
                p.message = Some(e);
                emit_job(app, p);
                continue;
            }
        };
        if wi.md_path.exists() {
            let mut p = payload(&wi.item.id, &wi.item.display_name, "skipped");
            p.message = Some(format!("Skipped (exists): {}", wi.md_path.display()));
            emit_job(app, p);
            continue;
        }
        work.push(wi);
    }

    let total = work.len();
    if total == 0 {
        return (0, Ok(()));
    }

    let done_counter = Arc::new(AtomicUsize::new(0));

    let (tx, mut rx) = tokio::sync::mpsc::channel::<TranscribedJob>(1);

    let app_w = app.clone();
    let cfg_w = cfg.clone();
    let done_w = done_counter.clone();
    let whisper_task = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let ctx_params = WhisperContextParameters {
            use_gpu: crate::vulkan_runtime::gpu_usable() && cfg_w.use_gpu,
            ..Default::default()
        };

        let model_path_str = model_path
            .to_str()
            .ok_or_else(|| "Whisper model path contains non-UTF-8 characters".to_string())?;
        let ctx = WhisperContext::new_with_params(model_path_str, ctx_params)
            .map_err(|e| format!("Whisper init: {e}"))?;

        for (idx, wi) in work.iter().enumerate() {
            if cancel_requested() {
                emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                break;
            }

            let id = wi.item.id.clone();
            let display_name = wi.item.display_name.clone();

            // Podcast episodes: download into the chosen output folder (same stem as the .md).
            let audio_path: PathBuf = if wi.item.is_podcast() {
                if wi.local_audio.exists() {
                    wi.local_audio.clone()
                } else {
                    let mut p = payload(&id, &display_name, "download");
                    p.download_pct = Some(0);
                    p.overall = Some(overall_snapshot(&done_w, total));
                    emit_job(&app_w, p);

                    let app_p = app_w.clone();
                    let id_p = id.clone();
                    let disp_p = display_name.clone();
                    let last_pct = AtomicI32::new(-1);
                    let dest = wi.local_audio.clone();
                    let dl = podcast::download_to_file_blocking(
                        &wi.item.source,
                        &dest,
                        move |dl, total_b| {
                            // No Content-Length means no meaningful percentage.
                            let Some(pct) = (dl * 100).checked_div(total_b) else {
                                return;
                            };
                            let pct = pct as i32;
                            if last_pct.swap(pct, Ordering::Relaxed) == pct {
                                return;
                            }
                            let mut p = payload(&id_p, &disp_p, "download");
                            p.download_pct = Some(pct);
                            emit_job(&app_p, p);
                        },
                    );
                    match dl {
                        Ok(()) => dest,
                        Err(e) => {
                            // A cancel mid-download surfaces here; report the whole
                            // remaining tail as cancelled rather than as one error.
                            if cancel_requested() {
                                emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                                break;
                            }
                            emit_error(&app_w, &id, &display_name, e, &done_w, total);
                            continue;
                        }
                    }
                }
            } else {
                wi.local_audio.clone()
            };

            if cancel_requested() {
                emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                break;
            }

            let mut p = payload(&id, &display_name, "whisper");
            p.whisper_pct = Some(0);
            p.overall = Some(overall_snapshot(&done_w, total));
            emit_job(&app_w, p);

            // Whisper reports whole percent; dedupe so one event per percent reaches the UI.
            let on_progress = {
                let app_pg = app_w.clone();
                let id_pg = id.clone();
                let disp_pg = display_name.clone();
                let mut last = 0i32;
                move |pct: i32| {
                    if pct == last {
                        return;
                    }
                    last = pct;
                    let mut p = payload(&id_pg, &disp_pg, "whisper");
                    p.whisper_pct = Some(pct);
                    emit_job(&app_pg, p);
                }
            };

            let raw = match transcribe_one(&ctx, &audio_path, &cfg_w, on_progress) {
                Ok(t) => t,
                Err(e) => {
                    if cancel_requested() {
                        emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                        break;
                    }
                    emit_error(&app_w, &id, &display_name, e, &done_w, total);
                    continue;
                }
            };

            if raw.trim().is_empty() {
                emit_error(
                    &app_w,
                    &id,
                    &display_name,
                    "No speech detected.".to_string(),
                    &done_w,
                    total,
                );
                continue;
            }

            if cancel_requested() {
                emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                break;
            }

            let job = TranscribedJob {
                work: wi.clone(),
                raw_text: raw,
            };

            if tx.blocking_send(job).is_err() {
                break;
            }
        }
        drop(tx);
        Ok(())
    });

    let app_l = app.clone();
    let cfg_l = cfg;
    let done_llm = done_counter.clone();
    let llm_task = tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            llm_stage(app_l.clone(), cfg_l.clone(), job, done_llm.clone(), total).await;
        }
    });

    // Always await the LLM task, including on the error paths below: detaching it
    // let `job_progress` events arrive after `batch_complete` and revive rows the
    // frontend had already settled. The sender is dropped either way, so the
    // receive loop terminates on its own.
    let wh_res = whisper_task.await;
    let _ = llm_task.await;

    match wh_res {
        // Panic inside the blocking task. This used to bail out with `?`, leaving
        // PROCESSING set forever; the guard in `run_batch` now covers it regardless.
        Err(e) => (total, Err(format!("Transcription task failed: {e}"))),
        Ok(Err(e)) => (total, Err(e)),
        Ok(Ok(())) => (total, Ok(())),
    }
}
