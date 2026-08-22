use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::{downmix_into, resampler_to_16k, SAMPLE_RATE};
use crate::config::AppConfig;
use crate::model_download;
use crate::pipeline;

const INTERVAL: Duration = Duration::from_millis(1200);
const LEVEL_INTERVAL: Duration = Duration::from_millis(50);
const MIN_AUDIO: f32 = 0.7;
const TAIL_SIL: f32 = 0.8;
const MIN_COMMIT: f32 = 1.6;
const MAX_BUFFER: f32 = 25.0;

static DICTATING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
/// Latest input RMS as `f32::to_bits`. The audio callback only stores here; the
/// polling loop emits it. Serialising JSON and crossing the IPC boundary from a
/// realtime callback is what causes capture dropouts.
static LEVEL_BITS: AtomicU32 = AtomicU32::new(0);
static JOIN: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);
static MONITOR_STOP: AtomicBool = AtomicBool::new(false);
static MONITOR: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);

fn join_lock() -> std::sync::MutexGuard<'static, Option<thread::JoinHandle<()>>> {
    JOIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn monitor_lock() -> std::sync::MutexGuard<'static, Option<thread::JoinHandle<()>>> {
    MONITOR.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_running() -> bool {
    DICTATING.load(Ordering::SeqCst)
}

fn store_level(rms: f32) {
    LEVEL_BITS.store(rms.to_bits(), Ordering::Relaxed);
}

fn emit_level(app: &AppHandle) {
    let rms = f32::from_bits(LEVEL_BITS.load(Ordering::Relaxed));
    let _ = app.emit("dictation_level", serde_json::json!({ "rms": rms }));
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneInfo {
    pub name: String,
    pub is_default: bool,
}

pub fn list_microphones() -> Result<Vec<MicrophoneInfo>, String> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();
    let devices = host
        .input_devices()
        .map_err(|e| format!("List microphones: {e}"))?;
    let mut out = Vec::new();
    for d in devices {
        let Ok(name) = d.name() else { continue };
        let is_default = name == default_name;
        out.push(MicrophoneInfo { name, is_default });
    }
    if out.is_empty() && !default_name.is_empty() {
        out.push(MicrophoneInfo {
            name: default_name,
            is_default: true,
        });
    }
    Ok(out)
}

pub async fn start(app: AppHandle, cfg: AppConfig) -> Result<(), String> {
    // Claim first, then check: `start_transcription` claims its own slot before
    // testing `is_running()`, so one of the two always sees the other's flag and
    // the pair can never both hold a WhisperContext.
    if DICTATING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Dictation is already running.".to_string());
    }
    if pipeline::is_processing() {
        DICTATING.store(false, Ordering::SeqCst);
        return Err("A batch is already running.".to_string());
    }
    STOP.store(false, Ordering::SeqCst);

    let model_name = cfg.dictation_model.clone();
    let _ = app.emit(
        "dictation_status",
        serde_json::json!({ "stage": "loading", "message": "Loading dictation model…" }),
    );
    let model_path = match model_download::resolve_model(&model_name, |_, _| {}).await {
        Ok(p) => p,
        Err(e) => {
            DICTATING.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    let model_path = match model_path.to_str() {
        Some(s) => s.to_string(),
        None => {
            DICTATING.store(false, Ordering::SeqCst);
            return Err("Whisper model path contains non-UTF-8 characters".to_string());
        }
    };

    if let Some(h) = join_lock().take() {
        let _ = h.join();
    }

    let mic = cfg.microphone_name.clone();
    let language = cfg.language.clone();
    let app_t = app.clone();
    let handle = thread::Builder::new()
        .name("voxmd-dictation".into())
        .spawn(move || {
            if let Err(e) = run_loop(app_t.clone(), &model_path, &mic, &language) {
                let _ = app_t.emit(
                    "dictation_status",
                    serde_json::json!({ "stage": "error", "message": e }),
                );
            }
            DICTATING.store(false, Ordering::SeqCst);
            let _ = app_t.emit(
                "dictation_status",
                serde_json::json!({ "stage": "stopped" }),
            );
        })
        .map_err(|e| {
            DICTATING.store(false, Ordering::SeqCst);
            format!("Dictation thread: {e}")
        })?;
    *join_lock() = Some(handle);
    Ok(())
}

pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// Live input meter without Whisper. No-op while dictation owns the microphone.
pub fn start_monitor(app: AppHandle, microphone_name: String) -> Result<(), String> {
    let mut slot = monitor_lock();
    if DICTATING.load(Ordering::SeqCst) {
        return Ok(());
    }
    MONITOR_STOP.store(true, Ordering::SeqCst);
    if let Some(h) = slot.take() {
        let _ = h.join();
    }
    MONITOR_STOP.store(false, Ordering::SeqCst);
    let handle = thread::Builder::new()
        .name("voxmd-mic-monitor".into())
        .spawn(move || {
            if let Err(e) = run_monitor(app.clone(), &microphone_name) {
                let _ = app.emit(
                    "dictation_status",
                    serde_json::json!({
                        "stage": "idle",
                        "message": format!("Microphone: {e}")
                    }),
                );
            }
            store_level(0.0);
            let _ = app.emit("dictation_level", serde_json::json!({ "rms": 0.0 }));
        })
        .map_err(|e| format!("Microphone monitor: {e}"))?;
    *slot = Some(handle);
    Ok(())
}

pub fn stop_monitor() {
    MONITOR_STOP.store(true, Ordering::SeqCst);
    if let Some(h) = monitor_lock().take() {
        let _ = h.join();
    }
}

fn pick_device(preferred: &str) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if !preferred.trim().is_empty() {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.name().ok().as_deref() == Some(preferred)) {
                return Ok(d);
            }
        }
    }
    host.default_input_device()
        .ok_or_else(|| "No default microphone found.".to_string())
}

struct CaptureBuf {
    samples: Vec<f32>,
    rate: u32,
    channels: usize,
}

fn run_monitor(app: AppHandle, mic: &str) -> Result<(), String> {
    let device = pick_device(mic)?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("Microphone config: {e}"))?;
    let err_flag = Arc::new(Mutex::new(None::<String>));
    let stream = build_input_stream(&device, &supported, None, err_flag.clone())?;
    stream
        .play()
        .map_err(|e| format!("Start microphone: {e}"))?;
    while !MONITOR_STOP.load(Ordering::SeqCst) {
        if let Some(e) = err_flag.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Err(e);
        }
        emit_level(&app);
        thread::sleep(LEVEL_INTERVAL);
    }
    Ok(())
}

fn run_loop(app: AppHandle, model_path: &str, mic: &str, language: &str) -> Result<(), String> {
    stop_monitor();
    let device = pick_device(mic)?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("Microphone config: {e}"))?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;

    let buf = Arc::new(Mutex::new(CaptureBuf {
        samples: Vec::new(),
        rate: sample_rate,
        channels,
    }));
    let err_flag = Arc::new(Mutex::new(None::<String>));

    let stream = build_input_stream(&device, &supported, Some(buf.clone()), err_flag.clone())?;
    stream
        .play()
        .map_err(|e| format!("Start microphone: {e}"))?;

    let ctx = WhisperContext::new_with_params(
        model_path,
        WhisperContextParameters {
            use_gpu: false,
            ..Default::default()
        },
    )
    .map_err(|e| format!("Whisper init: {e}"))?;

    let _ = app.emit(
        "dictation_status",
        serde_json::json!({ "stage": "listening", "message": "Listening…" }),
    );

    let mut floor = 0.005f32;
    let mut last = Instant::now();
    let mut leftover: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    // One resampler for the whole session: a per-chunk instance would restart
    // the anti-alias biquads and the fractional read position every 1.2 s.
    let mut resampler = resampler_to_16k(sample_rate);

    while !STOP.load(Ordering::SeqCst) {
        if let Some(e) = err_flag.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Err(format!("Microphone: {e}"));
        }
        emit_level(&app);
        thread::sleep(LEVEL_INTERVAL);
        if last.elapsed() < INTERVAL {
            continue;
        }
        last = Instant::now();

        let chunk = {
            let mut g = buf.lock().unwrap_or_else(|e| e.into_inner());
            if g.samples.is_empty() {
                None
            } else {
                Some((std::mem::take(&mut g.samples), g.rate, g.channels))
            }
        };
        if let Some((raw, rate, ch)) = chunk {
            downmix_into(&raw, ch.max(1), &mut mono)?;
            match (&mut resampler, rate == SAMPLE_RATE) {
                (Some(r), false) => {
                    r.push(&mono);
                    leftover.extend(r.take());
                }
                _ => leftover.extend_from_slice(&mono),
            }
        }

        let n = leftover.len();
        let min_samples = (MIN_AUDIO * SAMPLE_RATE as f32) as usize;
        if n < min_samples {
            continue;
        }

        let text =
            match transcribe_buffer(&ctx, &leftover, language, || STOP.load(Ordering::SeqCst)) {
                Ok(t) => t,
                Err(_) if STOP.load(Ordering::SeqCst) => break,
                Err(e) => return Err(e),
            };
        let tail_n = ((TAIL_SIL * SAMPLE_RATE as f32) as usize).min(n);
        let tail = &leftover[n - tail_n..];
        let rms_tail = rms(tail);
        floor = rms_tail.max(1e-4).min(floor * 1.05 + 1e-5);
        let silent = rms_tail < 0.006f32.max(3.0 * floor);
        let long_enough = n > (MIN_COMMIT * SAMPLE_RATE as f32) as usize;
        let overflow = n > (MAX_BUFFER * SAMPLE_RATE as f32) as usize;

        if (silent && long_enough) || overflow {
            leftover.drain(..n);
            if !text.is_empty() {
                let _ = app.emit("dictation_final", serde_json::json!({ "text": text }));
            } else {
                let _ = app.emit("dictation_partial", serde_json::json!({ "text": "" }));
            }
        } else {
            let _ = app.emit("dictation_partial", serde_json::json!({ "text": text }));
        }
    }

    drop(stream);
    store_level(0.0);
    let _ = app.emit("dictation_level", serde_json::json!({ "rms": 0.0 }));

    // Whatever is still buffered when Stop is pressed is up to MAX_BUFFER
    // seconds of speech the user already said. This used to be guarded by
    // `!STOP`, which a normal stop always sets, so the tail was silently
    // dropped; the abort callback also has to ignore STOP here or whisper
    // returns immediately.
    if leftover.len() > SAMPLE_RATE as usize / 2 {
        let _ = app.emit(
            "dictation_status",
            serde_json::json!({ "stage": "finalizing", "message": "Finalizing…" }),
        );
        if let Ok(text) = transcribe_buffer(&ctx, &leftover, language, || false) {
            if !text.is_empty() {
                let _ = app.emit("dictation_final", serde_json::json!({ "text": text }));
            }
        }
    }
    Ok(())
}

fn build_input_stream(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    buf: Option<Arc<Mutex<CaptureBuf>>>,
    err_flag: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String> {
    let config = supported.config();
    match supported.sample_format() {
        SampleFormat::F32 => open_stream::<f32>(device, &config, buf, err_flag),
        SampleFormat::F64 => open_stream::<f64>(device, &config, buf, err_flag),
        SampleFormat::I8 => open_stream::<i8>(device, &config, buf, err_flag),
        SampleFormat::I16 => open_stream::<i16>(device, &config, buf, err_flag),
        SampleFormat::I32 => open_stream::<i32>(device, &config, buf, err_flag),
        SampleFormat::U8 => open_stream::<u8>(device, &config, buf, err_flag),
        SampleFormat::U16 => open_stream::<u16>(device, &config, buf, err_flag),
        SampleFormat::U32 => open_stream::<u32>(device, &config, buf, err_flag),
        other => Err(format!("Unsupported microphone sample format: {other}")),
    }
}

fn open_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Option<Arc<Mutex<CaptureBuf>>>,
    err_flag: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if let Some(buf) = &buf {
                    let mut g = buf.lock().unwrap_or_else(|e| e.into_inner());
                    g.samples
                        .extend(data.iter().copied().map(|s| s.to_sample::<f32>()));
                }
                store_level(rms_of(data));
            },
            move |err| {
                *err_flag.lock().unwrap_or_else(|e| e.into_inner()) = Some(err.to_string());
            },
            None,
        )
        .map_err(|e| format!("Open microphone: {e}"))
}

fn rms_of<T>(data: &[T]) -> f32
where
    T: SizedSample,
    f32: FromSample<T>,
{
    if data.is_empty() {
        return 0.0;
    }
    let sum: f32 = data
        .iter()
        .copied()
        .map(|s| {
            let x = s.to_sample::<f32>();
            x * x
        })
        .sum();
    (sum / data.len() as f32).sqrt()
}

fn transcribe_buffer(
    ctx: &WhisperContext,
    samples: &[f32],
    language: &str,
    abort: impl Fn() -> bool + 'static,
) -> Result<String, String> {
    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let lang = language.trim();
    let lang = if lang.is_empty() || lang.eq_ignore_ascii_case("auto") {
        "auto"
    } else {
        lang
    };
    params.set_language(Some(lang));
    params.set_n_threads(
        std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).max(1))
            .unwrap_or(2) as i32,
    );
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    type AbortCb = Box<dyn FnMut() -> bool>;
    params.set_abort_callback_safe::<Option<AbortCb>, AbortCb>(Some(Box::new(abort)));
    state
        .full(params, samples)
        .map_err(|e| format!("Whisper inference: {e}"))?;
    let n = state.full_n_segments();
    let mut parts = Vec::new();
    for i in 0..n {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let t = seg.to_str_lossy().unwrap_or_default().trim().to_string();
        if !t.is_empty() {
            parts.push(t);
        }
    }
    Ok(parts.join(" ").trim().to_string())
}

fn rms(xs: &[f32]) -> f32 {
    rms_of(xs)
}

#[cfg(test)]
mod tests {
    use super::rms;

    #[test]
    fn rms_of_zeros_is_zero() {
        assert_eq!(rms(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn rms_of_ones_is_one() {
        assert!((rms(&[1.0, -1.0]) - 1.0).abs() < 1e-6);
    }
}
