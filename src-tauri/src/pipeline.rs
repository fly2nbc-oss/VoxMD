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
    get_audio_metadata, get_audio_path_for_episode, get_md_path, get_md_path_for_episode,
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

pub fn request_cancel() {
    if PROCESSING.load(Ordering::SeqCst) {
        CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    }
}

fn cancel_requested() -> bool {
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

fn transcribe_one(
    ctx: &WhisperContext,
    audio_path: &Path,
    cfg: &AppConfig,
) -> Result<String, String> {
    let samples = decode_file_to_mono_16k(audio_path)?;

    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let lang = cfg.language.trim();
    let lang = if lang.is_empty() { "auto" } else { lang };
    params.set_language(Some(lang));
    params.set_n_threads(whisper_threads() as i32);
    state
        .full(params, &samples)
        .map_err(|e| format!("Whisper inference: {e}"))?;

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
        let md_path = get_md_path_for_episode(&out_dir, &ep.title, &ep.date);
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
                let mut p = payload(&id, &display_name, "error");
                p.overall = Some(overall_snapshot(&done_counter, total));
                p.message = Some(format!("Summary: {e}"));
                emit_job(&app, p);
                return;
            }
        }
    } else {
        String::new()
    };

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
    if let Err(e) = std::fs::write(md_path, content) {
        let mut p = payload(&id, &display_name, "error");
        p.overall = Some(overall_snapshot(&done_counter, total));
        p.message = Some(format!("Save failed: {e}"));
        emit_job(&app, p);
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
    p.overall = Some(OverallProgress {
        completed: c,
        total,
        pct: if total > 0 {
            (c as f32 / total as f32) * 100.0
        } else {
            100.0
        },
    });
    p.message = Some(format!("Saved: {}{}", md_path.display(), deletion_note));
    emit_job(&app, p);
}

fn overall_snapshot(done: &Arc<AtomicUsize>, total: usize) -> OverallProgress {
    let c = done.load(Ordering::SeqCst);
    OverallProgress {
        completed: c,
        total,
        pct: if total > 0 {
            (c as f32 / total as f32) * 100.0
        } else {
            0.0
        },
    }
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
pub async fn run_batch(
    app: AppHandle,
    items: Vec<QueueItem>,
    cfg: AppConfig,
) -> Result<(), String> {
    cfg.validate_for_run()?;

    if PROCESSING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Processing is already running.".to_string());
    }
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);

    // Resolve Whisper model (download from HuggingFace if name given and not cached).
    let model_path = {
        let app_dl = app.clone();
        let model_name = cfg.whisper_model.clone();
        let _ = app_dl.emit(
            "model_download_progress",
            serde_json::json!({ "stage": "resolving", "model": model_name }),
        );
        match model_download::resolve_model(&model_name.clone(), move |dl, total| {
            let pct = if total > 0 { dl * 100 / total } else { 0 };
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
            Err(e) => {
                CANCEL_REQUESTED.store(false, Ordering::SeqCst);
                PROCESSING.store(false, Ordering::SeqCst);
                return Err(e);
            }
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
                emit_job(&app, p);
                continue;
            }
        };
        if wi.md_path.exists() {
            let mut p = payload(&wi.item.id, &wi.item.display_name, "skipped");
            p.message = Some(format!("Skipped (exists): {}", wi.md_path.display()));
            emit_job(&app, p);
            continue;
        }
        work.push(wi);
    }

    let total = work.len();
    if total == 0 {
        CANCEL_REQUESTED.store(false, Ordering::SeqCst);
        PROCESSING.store(false, Ordering::SeqCst);
        let _ = app.emit("batch_complete", &serde_json::json!({ "total": 0u32 }));
        return Ok(());
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

        let n_items = work.len();
        for idx in 0..n_items {
            if cancel_requested() {
                emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                break;
            }

            let wi = work[idx].clone();
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
                            if total_b == 0 {
                                return;
                            }
                            let pct = (dl * 100 / total_b) as i32;
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
                            let mut p = payload(&id, &display_name, "error");
                            p.overall = Some(overall_snapshot(&done_w, total));
                            p.message = Some(e);
                            emit_job(&app_w, p);
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

            let raw = match transcribe_one(&ctx, &audio_path, &cfg_w) {
                Ok(t) => t,
                Err(e) => {
                    let mut p = payload(&id, &display_name, "error");
                    p.overall = Some(overall_snapshot(&done_w, total));
                    p.message = Some(e);
                    emit_job(&app_w, p);
                    continue;
                }
            };

            if raw.trim().is_empty() {
                let mut p = payload(&id, &display_name, "error");
                p.message = Some("No speech detected.".to_string());
                emit_job(&app_w, p);
                continue;
            }

            if cancel_requested() {
                emit_skipped_tail(&app_w, &work, idx, &done_w, total);
                break;
            }

            let job = TranscribedJob {
                work: wi,
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

    let wh_res = whisper_task.await.map_err(|e| e.to_string())?;
    if let Err(e) = wh_res {
        CANCEL_REQUESTED.store(false, Ordering::SeqCst);
        PROCESSING.store(false, Ordering::SeqCst);
        let _ = app.emit(
            "batch_complete",
            &serde_json::json!({ "total": total, "error": e }),
        );
        return Err(e);
    }

    llm_task.await.ok();

    let cancelled = cancel_requested();
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    PROCESSING.store(false, Ordering::SeqCst);
    let _ = app.emit(
        "batch_complete",
        &serde_json::json!({ "total": total, "cancelled": cancelled }),
    );
    Ok(())
}
