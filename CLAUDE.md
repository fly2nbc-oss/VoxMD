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

Unit tests live in-module (`#[cfg(test)]`) in `config.rs`, `llm.rs`, and `podcast.rs`; there are no frontend tests.

CI (`.github/workflows/ci.yml`) runs a Linux lint/test gate (tsc, fmt, clippy, cargo test) and then builds **with `--features gpu-vulkan` on both Linux and Windows**, so Vulkan headers must be present in any environment mirroring CI. Tags `v*` trigger `tauri-release.yml`.

## Architecture

### The bounded two-stage pipeline (the core design)

`pipeline.rs::run_batch` is the heart of the app. It runs **two concurrent tasks** connected by an mpsc channel of **capacity 1**:

- **Whisper task** (`spawn_blocking`): owns a single `WhisperContext`, processes queue items sequentially. For podcast items it downloads the episode into the chosen **output folder** (`podcast::download_to_file_blocking` → `meta::get_audio_path_for_episode`, same stem as the `.md`, stage `download`), then transcribes and sends each `TranscribedJob` into the channel. If the audio file already exists, download is skipped.
- **LLM task** (`tokio::spawn`): receives jobs, optionally runs the summary call, assembles the `.md` (meta block / summary / transcript per config toggles), writes it, and optionally deletes **audio only** (`local_audio`) if `delete_source_after_success` is set — **never** the Markdown.

Channel capacity 1 is the invariant: **at most one Whisper job and one LLM job in flight at once**. While the LLM works on file *n*, Whisper may transcribe file *n+1* — never more. Don't widen the channel without understanding this contract (single `WhisperContext`, memory, and ordering all depend on it).

Concurrency control uses two global atomics: `PROCESSING` (guards against double-start via `compare_exchange`) and `CANCEL_REQUESTED` (cooperative cancel, checked at task boundaries — there is **no hard mid-inference or mid-download cancellation**). Both are reset on completion.

### Queue items: local files vs. podcast episodes

The queue is `Vec<QueueItem>` (defined in `podcast.rs`, mirrored in `src/types.ts`): `{ id, kind: "local"|"podcast", source, displayName, episode? }`. `id` is the local path or episode audio URL and keys both the frontend list and `job_progress` events (payload field is still named `path`). Podcast items carry `EpisodeMeta` (feed title, episode title, date, link, `outputDir`). `prepare_work_item` resolves each item to a `WorkItem` with `md_path` and `local_audio` — local files write `.md` next to the audio; episodes use `get_md_path_for_episode` / `get_audio_path_for_episode` under `outputDir`.

Frontend also persists `podcastRecents` (up to 10 `{ feedUrl, outputDir, feedTitle? }` pairs) in the same settings store for the Podcast dialog.

### Frontend ↔ backend contract

The only channel between sides is Tauri IPC. Two directions:

- **Commands** (`invoke`): registered in `lib.rs::run()` via `generate_handler!`. Key ones: `start_transcription` (takes `items: Vec<QueueItem>`), `cancel_transcription`, `fetch_podcast_feed`, `list_whisper_models`, `clear_whisper_cache`, `vulkan_status`, `system_summary_language`.
- **Events** (`app.emit` → `listen` in `App.tsx`): `job_progress` (stages `queued/download/whisper/llm/done/skipped/error`, payload `JobProgressPayload` with optional `downloadPct`), `model_download_progress` (resolving/downloading/ready), `batch_complete`.

`AppConfig` crosses the boundary as a single struct. The Rust side (`config.rs`) uses `#[serde(rename_all = "camelCase")]`, so the Rust `snake_case` fields map 1:1 to the TS `camelCase` fields in `src/types.ts` / `src/defaults.ts`. **When adding a setting, update all of: `config.rs`, `types.ts`, `defaults.ts`, and the settings UI in `App.tsx`.** Settings persist client-side via `@tauri-apps/plugin-store` (note the `whisperModelPath` → `whisperModel` serde alias for old stores).

UI layout (not all in the settings drawer):

- Toolbar: Files, Podcast, Remove, Start; Markdown toggles (meta/summary/transcript); delete-audio trash toggle; Settings; About.
- Settings sections: **Summary (LLM)**, **Transcription (Whisper)**, **Appearance** (System/Light/Dark).
- Start: if any queue rows are selected, only those are sent to `start_transcription`; otherwise the full queue.
- Languages: Whisper `language` is `"auto"` or ISO; summary `summaryLanguage` is `"system"` or ISO.

`AppConfig::validate_for_run()` is the single source of truth for input validation and is called both from `start_transcription` and inside `run_batch`. The summary only runs when `summary_enabled()` is true (`include_summary` AND a non-empty API key) — without a key it is **skipped silently**, not an error. Validation requires that the output is non-empty: `summary_enabled() || include_transcript`. API URL/model are only validated when the summary will actually run.

### Rust modules

| Module | Responsibility |
|---|---|
| `lib.rs` | Tauri command handlers + app builder. `main.rs` just calls `run()`. |
| `pipeline.rs` | The two-stage pipeline, progress events, cancellation, `.md` assembly (meta/summary/transcript toggles), optional audio deletion. |
| `llm.rs` | Summary prompt + call (`generate_summary`), async-openai client, Whisper segment → raw text. |
| `podcast.rs` | `QueueItem`/`EpisodeMeta` types, RSS/Atom feed parsing (`feed-rs`), lazy episode download to output folder (`download_to_file_blocking`). |
| `audio.rs` | Symphonia decode → mono f32 @ 16 kHz (linear resample) for whisper.cpp. |
| `meta.rs` | Audio tag reading (lofty), `.md` / podcast audio path derivation, filename sanitizing. |
| `model_download.rs` | Whisper model presets, HF download into `~/.cache/voxmd/whisper/`, cache listing/clearing. |
| `config.rs` | `AppConfig`, defaults, validation, summary-language resolution (`system` → OS locale → ISO 639-1). |
| `vulkan_runtime.rs` | Runtime Vulkan loader probe (`gpu_usable`); used by `vulkan_status`. |
| `vulkan-stub/` | Static stub linked instead of system `libvulkan` so missing loader does not block process start. |

### LLM usage (`llm.rs`)

There is **no LLM pass over the transcript** — the transcript section in the output is the raw Whisper text (`[HH:MM:SS] text` lines from `segments_to_raw_text`). The only LLM call is `generate_summary`: one request per file with a fixed Markdown outline (one-sentence summary, key arguments, data & facts, quotes citing `[HH:MM:SS]`), written in the resolved summary language. Input is truncated at 50k chars; sampling is fixed (temperature 0.3, 8192 max tokens — not user-configurable); podcast metadata (feed/episode/date) is passed as an orientation context block. Prompts are authored in **English** (so timestamps stay ASCII), but the LLM is instructed to write in the configured language.

### Output format

`pipeline.rs::llm_stage` assembles: `# {title}` + optional meta block (feed/episode info for podcasts, file name/year for local files) + optional summary + optional `## Transcript` with the raw Whisper text — each part gated by `include_meta` / `include_summary` / `include_transcript`. The `.md` filename derives from audio tags (`{year} - {title}` or `{title}`, sanitized) for local files and from `{YYYY} - {episode title}` for episodes. **Files whose `.md` already exists are skipped** — re-running a batch is idempotent.

## Gotchas

- Whisper exposes no fine-grained percentage; progress is stage-based (`download` has a percentage, `whisper` / `llm` do not).
- `gpu-vulkan` is opt-in; `use_gpu` only applies when the binary was built with that feature **and** the Vulkan loader is present at runtime (`vulkan_runtime::gpu_usable()`). Missing `libvulkan.so` no longer prevents startup (link stub + runtime probe).
- `delete_source_after_success` defaults to **false**. When enabled it deletes **`local_audio` only** (local files and downloaded podcast audio) — **never** the Markdown. Deletion failure is reported as a note, not a hard error.
- `whisper_model` accepts a preset name (`turbo`, `large-v3`, …) **or** a local path ending in `.bin` or `.gguf` (path detection in `config.rs::looks_like_whisper_path`). UI: preset dropdown or **Custom path…** + file picker.
- `podcast::download_to_file_blocking` uses `Handle::current().block_on` and must be called from a thread with a Tokio runtime context (true inside `spawn_blocking`).
- The summary is **skipped silently when no API key is set** (`summary_enabled()`), so the app runs fully offline; validation only fails if the transcript is also disabled (empty output).
- Whisper thread count is auto-detected (cores − 1); LLM sampling is fixed in `llm.rs` — neither is a setting anymore. Old stores with `temperature`/`maxTokens`/`whisperThreads` load fine (unknown fields ignored, dropped on next save).
