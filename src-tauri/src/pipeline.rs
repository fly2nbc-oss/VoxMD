use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::decode_file_to_mono_16k;
use crate::config::AppConfig;
use crate::llm::{self, make_client};
use crate::meta::{get_audio_metadata, get_md_path};
use crate::model_download;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgressPayload {
    pub path: String,
    pub display_name: String,
    /// queued, whisper, llm, done, skipped, error
    pub stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whisper_pct: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_chunk: Option<[usize; 2]>,
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

pub fn is_processing() -> bool {
    PROCESSING.load(Ordering::SeqCst)
}

fn emit_job(app: &AppHandle, p: JobProgressPayload) {
    let _ = app.emit("job_progress", &p);
}

fn whisper_threads(cfg: &AppConfig) -> usize {
    cfg.whisper_threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).max(1))
            .unwrap_or(4)
    })
}

fn transcribe_one(
    ctx: &WhisperContext,
    audio_path: &PathBuf,
    cfg: &AppConfig,
) -> Result<String, String> {
    let samples = decode_file_to_mono_16k(audio_path)?;

    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(&cfg.language));
    params.set_n_threads(whisper_threads(cfg) as i32);
    params.set_print_progress(cfg.whisper_verbose);
    params.set_print_realtime(cfg.whisper_verbose);

    state
        .full(params, &samples)
        .map_err(|e| format!("Whisper inference: {e}"))?;

    llm::segments_to_raw_text(&state)
}

pub struct TranscribedJob {
    pub path: PathBuf,
    pub display_name: String,
    pub meta_title: String,
    pub meta_year: Option<String>,
    pub raw_text: String,
}

async fn llm_stage(
    app: AppHandle,
    cfg: AppConfig,
    job: TranscribedJob,
    done_counter: Arc<AtomicUsize>,
    total: usize,
) {
    let path_str = job.path.display().to_string();
    let client = make_client(&cfg);

    emit_job(
        &app,
        JobProgressPayload {
            path: path_str.clone(),
            display_name: job.display_name.clone(),
            stage: "llm".to_string(),
            whisper_pct: None,
            llm_chunk: None,
            overall: None,
            message: Some("Speakers & summary…".to_string()),
        },
    );

    let path_s = path_str.clone();
    let disp = job.display_name.clone();

    let transcript_result = llm::transcript_with_speakers_with_progress(
        &client,
        &cfg,
        &job.raw_text,
        |cur, n| {
            emit_job(
                &app,
                JobProgressPayload {
                    path: path_s.clone(),
                    display_name: disp.clone(),
                    stage: "llm".to_string(),
                    whisper_pct: None,
                    llm_chunk: Some([cur, n]),
                    overall: None,
                    message: None,
                },
            );
        },
    )
    .await;

    let transcript = match transcript_result {
        Ok(t) => t,
        Err(e) => {
            emit_job(
                &app,
                JobProgressPayload {
                    path: path_str.clone(),
                    display_name: job.display_name.clone(),
                    stage: "error".to_string(),
                    whisper_pct: None,
                    llm_chunk: None,
                    overall: Some(overall_snapshot(&done_counter, total)),
                    message: Some(e),
                },
            );
            return;
        }
    };

    let summary = match llm::generate_summary(&client, &cfg, &transcript).await {
        Ok(s) => s,
        Err(e) => {
            emit_job(
                &app,
                JobProgressPayload {
                    path: path_str.clone(),
                    display_name: job.display_name.clone(),
                    stage: "error".to_string(),
                    whisper_pct: None,
                    llm_chunk: None,
                    overall: Some(overall_snapshot(&done_counter, total)),
                    message: Some(format!("Summary: {e}")),
                },
            );
            return;
        }
    };

    let mut summary_block = summary.trim().to_string();
    if !summary_block.is_empty() {
        summary_block.push_str("\n\n");
    }

    let md_path = get_md_path(&job.path, &job.meta_title, &job.meta_year);
    let content = format!(
        "# {}\n\n{summary_block}## Originaltranskript\n\n{transcript}\n",
        job.meta_title
    );

    if let Err(e) = std::fs::write(&md_path, content) {
        emit_job(
            &app,
            JobProgressPayload {
                path: path_str.clone(),
                display_name: job.display_name.clone(),
                stage: "error".to_string(),
                whisper_pct: None,
                llm_chunk: None,
                overall: Some(overall_snapshot(&done_counter, total)),
                message: Some(format!("Save failed: {e}")),
            },
        );
        return;
    }

    if cfg.delete_source_after_success {
        let _ = std::fs::remove_file(&job.path);
    }

    let c = done_counter.fetch_add(1, Ordering::SeqCst) + 1;
    emit_job(
        &app,
        JobProgressPayload {
            path: path_str.clone(),
            display_name: job.display_name.clone(),
            stage: "done".to_string(),
            whisper_pct: None,
            llm_chunk: None,
            overall: Some(OverallProgress {
                completed: c,
                total,
                pct: if total > 0 {
                    (c as f32 / total as f32) * 100.0
                } else {
                    100.0
                },
            }),
            message: Some(format!("Saved: {}", md_path.display())),
        },
    );
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

/// Parallele Pipeline: während LLM Datei _n_ bearbeitet, läuft Whisper auf _n+1_.
pub async fn run_batch(app: AppHandle, paths: Vec<PathBuf>, cfg: AppConfig) -> Result<(), String> {
    cfg.validate_for_run()?;

    if PROCESSING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Processing is already running.".to_string());
    }

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
                PROCESSING.store(false, Ordering::SeqCst);
                return Err(e);
            }
        }
    };
    let _ = app.emit(
        "model_download_progress",
        serde_json::json!({ "stage": "ready", "path": model_path.display().to_string() }),
    );

    let mut work: Vec<PathBuf> = Vec::new();
    for p in paths {
        let (title, year) = get_audio_metadata(&p);
        let md = get_md_path(&p, &title, &year);
        if md.exists() {
            emit_job(
                &app,
                JobProgressPayload {
                    path: p.display().to_string(),
                    display_name: p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string(),
                    stage: "skipped".to_string(),
                    whisper_pct: None,
                    llm_chunk: None,
                    overall: None,
                    message: Some(format!("Skipped (exists): {}", md.display())),
                },
            );
            continue;
        }
        work.push(p);
    }

    let total = work.len();
    if total == 0 {
        PROCESSING.store(false, Ordering::SeqCst);
        let _ = app.emit("batch_complete", &serde_json::json!({ "total": 0u32 }));
        return Ok(());
    }

    let done_counter = Arc::new(AtomicUsize::new(0));

    // Pipeline: Whisper (file N+1) and LLM (file N) run in parallel,
    // but never more than one of each. Channel capacity=1 enforces this.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TranscribedJob>(1);

    let app_w = app.clone();
    let cfg_w = cfg.clone();
    let done_w = done_counter.clone();
    let whisper_task = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu = cfg!(feature = "gpu-vulkan") && cfg_w.use_gpu;

        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or_default(),
            ctx_params,
        )
        .map_err(|e| format!("Whisper init: {e}"))?;

        for path in work {
            let (meta_title, meta_year) = get_audio_metadata(&path);
            let display_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            emit_job(
                &app_w,
                JobProgressPayload {
                    path: path.display().to_string(),
                    display_name: display_name.clone(),
                    stage: "whisper".to_string(),
                    whisper_pct: Some(0),
                    llm_chunk: None,
                    overall: Some(overall_snapshot(&done_w, total)),
                    message: None,
                },
            );

            let raw = match transcribe_one(&ctx, &path, &cfg_w) {
                Ok(t) => t,
                Err(e) => {
                    emit_job(
                        &app_w,
                        JobProgressPayload {
                            path: path.display().to_string(),
                            display_name: display_name.clone(),
                            stage: "error".to_string(),
                            whisper_pct: None,
                            llm_chunk: None,
                            overall: Some(overall_snapshot(&done_w, total)),
                            message: Some(e),
                        },
                    );
                    continue;
                }
            };

            if raw.trim().is_empty() {
                emit_job(
                    &app_w,
                    JobProgressPayload {
                        path: path.display().to_string(),
                        display_name: display_name.clone(),
                        stage: "error".to_string(),
                        whisper_pct: None,
                        llm_chunk: None,
                        overall: None,
                        message: Some("No speech detected.".to_string()),
                    },
                );
                continue;
            }

            let job = TranscribedJob {
                path,
                display_name,
                meta_title,
                meta_year,
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
        PROCESSING.store(false, Ordering::SeqCst);
        let _ = app.emit(
            "batch_complete",
            &serde_json::json!({ "total": total, "error": e }),
        );
        return Err(e);
    }

    llm_task.await.ok();

    PROCESSING.store(false, Ordering::SeqCst);
    let _ = app.emit(
        "batch_complete",
        &serde_json::json!({ "total": total }),
    );
    Ok(())
}
