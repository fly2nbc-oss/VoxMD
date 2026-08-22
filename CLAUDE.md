# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

VoxMD is a **Tauri v2** desktop app: a **React 19 + TypeScript** frontend (`src/`) and a **Rust** backend (`src-tauri/src/`). It transcribes local audio files and podcast episodes (RSS feeds) with **whisper.cpp** (via `whisper-rs`), optionally generates a structured summary with an **OpenAI-compatible LLM** (default Deepseek), and writes one Markdown file per source. The parent mono-repo `../CLAUDE.md` covers shared conventions; this file covers VoxMD specifics.

## Commands

```bash
npm install
npm run tauri dev        # Vite on port 1420 + Rust hot-reload (CPU-only; note: 1420, not the mono-repo default 5173)
npm run tauri build      # CPU-only release (default feature set)
npm run tauri:vulkan     # GPU build: ensures Vulkan SDK, then build --features gpu-vulkan (needs bash, e.g. Git Bash on Windows)
# Dev with GPU:
bash scripts/ensure-vulkan-sdk.sh && npx tauri dev --features gpu-vulkan
npm run build            # frontend only: tsc type-check + vite build
```

Rust checks (run from `src-tauri/`):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test resolve_explicit_iso_code   # single test
```

Unit tests live in-module (`#[cfg(test)]`) in `config.rs`, `llm.rs`, `pipeline.rs`, `model_download.rs`, `podcast.rs`, and others; frontend pure helpers under `src/lib/` are covered by Vitest (`npm test`).

CI (`.github/workflows/ci.yml`) runs a Linux lint/test gate (eslint, Vitest, tsc, fmt, clippy, cargo test) and then builds **with `--features gpu-vulkan` on both Linux and Windows**, so Vulkan headers must be present in any environment mirroring CI. Tags `v*` trigger `tauri-release.yml`.

## Architecture

### The bounded two-stage pipeline (the core design)

`pipeline.rs::run_batch` is the heart of the app. It runs **two concurrent tasks** connected by an mpsc channel of **capacity 1**:

- **Whisper task** (`spawn_blocking`): owns a single `WhisperContext`, pops items from a process-wide `PENDING` deque. For podcast items it downloads the episode into the chosen **output folder** (`podcast::download_to_file_blocking` → `meta::get_audio_path_for_episode`, same stem as the `.md`, stage `download`), then transcribes and sends each `TranscribedJob` into the channel. If the audio file already exists, download is skipped. Audio is decoded once (`decode_file_to_mono_16k`) and reused for optional diarization (stage `diarize`). The loop stays alive while the LLM still has a job so `append_to_batch` is not lost.
- **LLM task** (`tokio::spawn`): receives jobs, optionally runs the summary call, assembles the `.md` (meta block / summary / transcript per config toggles), writes it, and optionally deletes **audio only** (`local_audio`) if `delete_source_after_success` is set — **never** the Markdown.

Channel capacity 1 is the invariant: **at most one Whisper job and one LLM job in flight at once**. While the LLM works on file *n*, Whisper may transcribe file *n+1* — never more. Don't widen the channel without understanding this contract (single `WhisperContext`, memory, and ordering all depend on it). `total` on `overall` progress is reread from `BATCH_TOTAL` because the deque can grow.

Concurrency control uses two global atomics: `PROCESSING` (guards against double-start via `compare_exchange`) and `CANCEL_REQUESTED` (cooperative cancel, checked at task boundaries — there is **no hard mid-inference or mid-download cancellation**). Dictation (`DICTATING`) and a batch are mutually exclusive: each side claims **its own** flag first and then checks the other's, so the loser backs out (`pipeline::release_batch`) instead of both starting. Both batch flags are reset on completion. `keepawake` holds an idle-inhibit lock while `prevent_sleep` is set.

The pending queue is `Mutex<Pending { queue, closed }>`, not a bare deque. The Whisper loop may only exit through `try_finish_pending`, which sets `closed` under the same lock `append_to_batch` takes — otherwise an append acknowledged in that gap is discarded by `ProcessingGuard` without being processed.

### Queue items: local files vs. podcast episodes

The queue is `Vec<QueueItem>` (defined in `podcast.rs`, mirrored in `src/types.ts`): `{ id, kind: "local"|"podcast", source, displayName, episode? }`. `id` is the local path or episode audio URL and keys both the frontend list and `job_progress` events (payload field is still named `path`). Podcast items carry `EpisodeMeta` (feed title, episode title, date, link, `outputDir`). `prepare_work_item` resolves each item to a `WorkItem` with `md_path` and `local_audio` — local files write `.md` next to the audio; episodes use `get_md_path_for_episode` / `get_audio_path_for_episode` under `outputDir`.

Frontend also persists `podcastRecents` (up to 10 `{ feedUrl, outputDir, feedTitle? }` pairs) and unfinished `queueItems` in the same settings store. Completed (`done`) rows are not saved.

### Frontend ↔ backend contract

The only channel between sides is Tauri IPC. Two directions:

- **Commands** (`invoke`): registered in `lib.rs::run()` via `generate_handler!`. Key ones: `start_transcription` (takes `items: Vec<QueueItem>`), `append_to_batch`, `cancel_transcription`, `fetch_podcast_feed`, `list_whisper_models`, `clear_whisper_cache`, `vulkan_status`, `system_summary_language`, `list_llm_models`, `verify_api_key`, `improve_text`, `translate_text`, `list_microphones`, `start_dictation`, `stop_dictation`.
- **Events** (`app.emit` → `listen` in `App.tsx`): `job_progress` (stages `queued/download/whisper/diarize/llm/done/skipped/error`, payload `JobProgressPayload` with optional `downloadPct` and `outputPath`), `model_download_progress` (resolving/downloading/ready), `batch_complete`, plus `dictation_status` / `dictation_partial` / `dictation_final` / `dictation_level`.

`outputPath` is set on `done` and on a `Skipped (exists)` row. It is what lets the UI open the result without parsing the status text, and what tells an already-exported skip apart from a cancelled one in `queuePersist.ts`.

Batch events live in `useBatchEvents`, dictation events in `useDictationEvents` — **both at app level**. Dictation listeners must not sit in `DictationView`: leaving dictation mode unmounts it before the backend's `stopped` event arrives, which used to leave `running` stuck true.

Commands that hit the filesystem, enumerate audio devices, `dlopen` the Vulkan loader or join a thread are declared `#[tauri::command(async)]` so they do not block the main thread.

`AppConfig` crosses the boundary as a single struct. The Rust side (`config.rs`) uses `#[serde(rename_all = "camelCase")]`, so the Rust `snake_case` fields map 1:1 to the TS `camelCase` fields in `src/types.ts` / `src/defaults.ts`. **When adding a setting, update all of: `config.rs`, `types.ts`, `defaults.ts`, and the settings UI in `SettingsDrawer.tsx`.** Settings persist client-side via `@tauri-apps/plugin-store` (note the `whisperModelPath` → `whisperModel` serde alias for old stores). The processing queue is stored separately under `queueItems`.

UI layout (not all in the settings drawer):

- Toolbar: Queue/Dictation mode; Files, Podcast, Remove, Start; Markdown toggles (meta/summary/transcript); delete-audio trash toggle; Settings; About.
- Settings sections: **Summary (LLM)** (provider / key / URL / model), **Transcription (Whisper)** (including prevent-sleep and speaker labels), **Dictation**, **Appearance** (System/Light/Dark).
- Start: if any queue rows are selected, only those are sent to `start_transcription`; otherwise the full queue. While a batch runs, newly added files go to `append_to_batch`.
- Languages: Whisper `language` is `"auto"` or ISO; summary `summaryLanguage` is `"system"` or ISO.

`AppConfig::validate_for_run()` is the single source of truth for input validation and is called both from `start_transcription` and inside `run_batch`. The summary only runs when `summary_enabled()` is true (`include_summary` AND a non-empty API key) — without a key it is **skipped silently**, not an error. Validation requires that the output is non-empty: `summary_enabled() || include_transcript`. API URL/model are only validated when the summary will actually run.

### Rust modules

| Module | Responsibility |
|---|---|
| `lib.rs` | Tauri command handlers + app builder. `main.rs` just calls `run()`. |
| `pipeline.rs` | The two-stage pipeline, live pending deque, progress events, cancellation, `.md` assembly, optional audio deletion, prevent-sleep guard. |
| `llm.rs` | Summary / improve / translate prompts, `list_llm_models` / `verify_api_key`, Whisper segment → labeled transcript text. |
| `diarize.rs` | pyannote ONNX segmentation + CAM++ embeddings; own agglomerative clustering (not EmbeddingManager). Isolated so an engine swap is one file. |
| `onnx_runtime.rs` | Fetches and verifies the ONNX Runtime shared library `ort` dlopens, into the diarization cache. |
| `dictation.rs` | cpal capture, RMS silence detection, dedicated Whisper context, microphone list. |
| `podcast.rs` | `QueueItem`/`EpisodeMeta` types, RSS/Atom feed parsing (`feed-rs`), lazy episode download to output folder (`download_to_file_blocking`). |
| `audio.rs` | Symphonia decode → mono f32 @ 16 kHz (linear resample) for whisper.cpp and the diarizer. |
| `meta.rs` | Audio tag reading (lofty), `.md` / podcast audio path derivation, filename sanitizing. |
| `model_download.rs` | Whisper model presets, HF download into `~/.cache/voxmd/whisper/`, cache listing/clearing. |
| `config.rs` | `AppConfig`, defaults, validation, summary-language resolution (`system` → OS locale → ISO 639-1). |
| `vulkan_runtime.rs` | Runtime Vulkan loader probe (`gpu_usable`); used by `vulkan_status`. |
| `vulkan-stub/` | Static stub linked instead of system `libvulkan` so missing loader does not block process start. |

### LLM usage (`llm.rs`)

There is **no LLM pass over the batch transcript** — the transcript section in the output is Whisper text (`[HH:MM:SS] text`, or `[HH:MM:SS] **Speaker N:** text` when diarization is on) from `format_transcript`. The batch LLM call is `generate_summary`: one request per file when the transcript fits in 120k characters, otherwise map-reduce (part notes, then a final summary) with the same Markdown outline. Sampling is fixed (temperature 0.3, 8192 max tokens — not user-configurable); podcast metadata (feed/episode/date) is passed as an orientation context block. Prompts are authored in **English** (so timestamps stay ASCII), but the LLM is instructed to write in the configured language. Dictation can call `improve_text` / `translate_text` on the captured text only.

### Output format

`pipeline.rs::llm_stage` assembles: `# {title}` + optional meta block (feed/episode info for podcasts, file name/year for local files) + optional summary + optional `## Transcript` with the Whisper text — each part gated by `include_meta` / `include_summary` / `include_transcript`. The `.md` filename derives from audio tags (`{year} - {title}` or `{title}`, sanitized) for local files and from `{YYYY} - {episode title}` for episodes. **Files whose `.md` already exists are skipped** — re-running a batch is idempotent.

## Gotchas

- Whisper exposes no fine-grained percentage; progress is stage-based (`download` has a percentage, `whisper` / `diarize` / `llm` do not).
- Diarization's frame grid is **per 10 s window**: `frame_offset(window_start, frame)`. `segmentation-3.0` emits 589 frames of 270 samples for a 160 000-sample window, so a counter carried across windows loses 970 samples each time (~22 s per hour). Upstream pyannote-rs has this bug; `speech_segments` documents it as fixed point 5.
- `audio::Resampler` is stateful (biquads, fractional read position). Live capture must reuse **one** instance via `resampler_to_16k` — a fresh one per chunk puts a settling transient at every chunk boundary.
- The dictation tail must still be transcribed after `STOP` is set, so `transcribe_buffer` takes its abort predicate as a parameter rather than reading `STOP` itself.
- `MAX_SPEAKERS_CAP` (`config.rs`) and `MAX_SPEAKERS` (`src/lib/configStore.ts`) are checked against each other by a Rust test, as are the two `AUDIO_EXTENSIONS` lists.
- `cpal` links `libasound.so.2` and `keepawake` links `libdbus-1.so.3`; both are declared in `bundle.linux.deb.depends`.
- `gpu-vulkan` is opt-in; `use_gpu` only applies when the binary was built with that feature **and** the Vulkan loader is present at runtime (`vulkan_runtime::gpu_usable()`). Missing `libvulkan.so` no longer prevents startup (link stub + runtime probe).
- `delete_source_after_success` defaults to **false**. When enabled it deletes **`local_audio` only** (local files and downloaded podcast audio) — **never** the Markdown. Deletion failure is reported as a note, not a hard error.
- `whisper_model` accepts a preset name (`turbo`, `large-v3`, …) **or** a local path ending in `.bin` or `.gguf` (path detection in `config.rs::looks_like_whisper_path`). UI: preset dropdown or **Custom path…** + file picker. Dictation uses `dictation_model` the same way.
- `podcast::download_to_file_blocking` uses `Handle::current().block_on` and must be called from a thread with a Tokio runtime context (true inside `spawn_blocking`).
- The summary is **skipped silently when no API key is set** (`summary_enabled()`), so the app runs fully offline; validation only fails if the transcript is also disabled (empty output).
- Whisper thread count is auto-detected (cores − 1); LLM sampling is fixed in `llm.rs` — neither is a setting anymore. Old stores with `temperature`/`maxTokens`/`whisperThreads` load fine (unknown fields ignored, dropped on next save).
- `ort` must stay at `=2.0.0-rc.10` until `pyannote-rs` pins it; later rcs do not compile against this crate.
- **onnxruntime is not linked in.** `ort` uses `load-dynamic`, and `onnx_runtime.rs` downloads the matching Microsoft release (pinned version + SHA-256 per target) on first diarized run. That took the CPU binary from 39.4 MB to 19.1 MB and the released Vulkan binary from 75.9 MB to 55.6 MB, for a feature that is off by default. `ort::MINOR_VERSION` is checked against the pinned release by a `const` assertion, so an `ort` bump fails the build until the assets and hashes are updated.
- `ort` **panics** when it cannot open its dylib. Every path into a `Session` must go through `onnx_runtime::init`, which checks the file first and returns a normal error — diarization then degrades to the unlabeled transcript.
- `alternative-backend` (→ `ort-sys/disable-linking`) is on because `download-binaries` still arrives through `pyannote-rs`'s `ort` defaults, which a dependent cannot switch off. Without it every clean build fetches a 94 MB static archive it never links.
- **All runtime downloads land in the directory they are loaded from.** Whisper models: `~/.cache/voxmd/whisper/` (`model_download::cache_dir`). Diarization — both ONNX models *and* `libonnxruntime.so` / `onnxruntime.dll` — `~/.cache/voxmd/diarize/` (`diarize::cache_dir`, reached only via `diarize::cached` / `onnx_runtime::library_path`). Tests in both modules pin this.
