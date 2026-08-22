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

### UI language (`src/i18n/`)

`en.ts` is the source of truth: `Messages = Record<keyof typeof en, string>`, so `de/fr/it/es` are type errors until they cover exactly the same keys. `i18n.test.ts` additionally checks that no entry is blank and that every locale keeps the same `{placeholder}` set. `format()` leaves an unmatched placeholder visible rather than blanking it.

`App` builds the value with `useI18nValue(config.uiLanguage)` and puts it on `I18nContext`. **`useBatchEvents` and `useNativeDrop` take `t` as a parameter** — they run inside `App`, which is the component providing the context, so `useT()` there would read the English default. `ErrorBoundary` sits outside the provider and resolves the locale from `navigator` directly, since the settings store is exactly what may have failed.

Backend messages (Whisper, the LLM call, file paths) stay English: they cross IPC as free text. `detailsForRow` translates only the placeholder shown *before* a backend message arrives.

`AppConfig` crosses the boundary as a single struct. The Rust side (`config.rs`) uses `#[serde(rename_all = "camelCase")]`, so the Rust `snake_case` fields map 1:1 to the TS `camelCase` fields in `src/types.ts` / `src/defaults.ts`. **When adding a setting, update all of: `config.rs`, `types.ts`, `defaults.ts`, and the settings UI in `SettingsDrawer.tsx`.** Settings persist client-side via `@tauri-apps/plugin-store` (note the `whisperModelPath` → `whisperModel` serde alias for old stores). The processing queue is stored separately under `queueItems`.

UI layout (not all in the settings drawer):

- Toolbar: Queue/Dictation mode; Files, Podcast, Remove, Start; Markdown toggles (meta/summary/transcript); delete-audio trash toggle; Settings.
- Queue view, top to bottom: `ErrorPanel` (failures, untruncated), `ActiveJobs`, `QueueTable`, `StatusBar`.
- **`ActiveJobs` shows up to TWO entries, and that is the pipeline's contract, not a display choice** — the mpsc channel of capacity 1 lets Whisper run one file ahead of the summary. Anything that renders "the current job" as one entry is wrong. Only `download` and `whisper` carry a percentage (`jobPercent`); `diarize` and `llm` get an indeterminate marker rather than a fabricated bar.
- `QueueTable` has no Details column: for a waiting row it repeated the badge ("Wait" / "Waiting in queue…"), and for an active one the band now says it. Names are one ellipsised line (`.queue-name` needs `max-width: 0` for `text-overflow` to apply in a table cell) — wrapped podcast titles used to halve how many rows fit.
- The footer counts in words via `queueCounts` ("99 in der Warteschlange · 1 fertig"). It used to read "Overall: 0 / 1 done (MD)", which counted only the running batch and so contradicted a queue holding ninety-nine.
- Settings is a **wide drawer with a search rail** (`min(1024px, 100%)`). Rail order is Appearance / Transcription / Summary / Dictation, then **About** below a divider — the About dialog was folded in, there is no separate About surface or toolbar button. `.settings-body` is `overflow: hidden` on purpose, so a field that no longer fits shows up immediately instead of silently reintroducing a scroll; `.settings-panel` keeps `auto` as the safety valve for very short windows.
- Speakers is part of **Transcription**: both describe what falls out of the same local pass over the audio.
- Each rail row shows its current value, and a dot when that section holds an unsaved edit. The footer counts unsaved fields. This is what `useConfigStore` exposes `saved` for — the last-persisted config, diffed against the live one by `changedFields` / `changedSections`.
- The search index lives in `src/lib/settingsSearch.ts`, not in the component: a plain array of `{id, section, field, label, hint, keywords}` plus a pure `searchSettings(query, t)`. Matching folds diacritics and requires every term, against the *translated* label and hint plus untranslated keywords — so "vulkan" finds the GPU row whose label never says Vulkan, and "schlussel" finds "Schlüssel". A row's `field` drives both the unsaved dot and the ring after a search jump; rows without a stored setting (the model cache) omit it.
- Appearance holds the theme *and* the interface language (`uiLanguage`: `system` or one of `en/de/fr/it/es`).
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
- **`segmentation-3.0` must be run with overlapping windows.** It numbers speakers *within* a window, so index 1 in one window and index 1 in the next are unrelated. `speech_segments` hops by 50 % and picks the best of the six index permutations on the shared frames. Cutting every turn at the boundary instead — what the code did until 1.1.0 — put 62 % of all turn boundaries on a multiple of ten, where chance is 4 %; a continuous monologue came out as alternating speakers. After the fix: 3 %.
- **Every embedded excerpt must be the same length, and must be one unbroken stretch of audio.** This was the root cause behind every diarization failure in this module, and both halves of it were invisible until measured with `diarize::tests::anchor_purity`. (1) *Length dominates identity*: CAM++ places a 4.8 s excerpt 0.96 away from a 9.9 s excerpt **of the same person** — further than two different people ever land. With anchors ranging 1.5–10 s, the principal axis of the embedding space was duration, not voice. (2) *Splices read as a voice change*: cutting the inactive frames out and concatenating what remains puts a step discontinuity at each junction, and the same speaker's spliced windows sat 0.42–0.58 from their contiguous ones. Together these produced eight clusters for two speakers, and a forced `k = 2` scored 55.3 % against a 52.6 % baseline — the top split was spliced-vs-contiguous, not person-vs-person. The fix is `EMBED_LENS` + `embed_span`: keep pauses shorter than `MAX_EMBED_GAP_S` inside the audio, end the stretch where the other voice starts, and hand the embedder exactly 6 s centred in it (4 s where nobody speaks that long). No threshold can substitute for this — measure the anchors before touching anything else.
- **Tune `CLUSTER_DIST` by measuring the anchors, never by fitting the cluster count.** `anchor_purity` embeds the units clustering actually receives and prints within/between distances: at a fixed 6 s excerpt, same voice ≤0.20 and two voices ≥0.71; at 4 s, ≤0.32 and ≥0.69. 0.45 sits in both empty bands. `embedding_distance_matrix` does the same for hand-cut excerpts — useful to confirm two stretches really are two people, but it flatters the pipeline: it was reporting a clean 0.07–0.18 / 0.55–0.67 split throughout the period when the real anchors were spreading to 0.99 within one speaker. Fitting the threshold against bad anchors is hopeless at *any* value; fix the anchors first.
- **Cluster one unit per (window, local speaker), never long runs.** Inside a 10 s window, `segmentation-3.0`'s local speaker index *is* one speaker — the audio behind it is single-speaker by construction. Windows overlap 50 %, a speaker crossing a boundary contributes a unit on each side, and clustering ties them back together; frame labels come from weighted votes across the overlapping windows. Stitching windows into long runs first and embedding those looked reasonable and was the single biggest error: a run follows track activity, track identity drifts wherever only one person is speaking, and about half the resulting anchors sat *between* the two voices. That is what produced five clusters for two speakers, and no threshold could repair it. Measured effect of the change: 84.1 % → 90.7 %.
- **Measure diarization with `diarize::quality::accuracy_against_reference`, never by eye.** It builds a reference from the embedder alone (hand-verified voice samples → per-line nearest match) and reports the majority-class rate alongside, so a number a constant predictor could reach is visible as one. **A reference line needs at least 6 s of audio**, not the 1.5 s an anchor needs. At 1.5 s the per-line embeddings are noise: one speaker's three-minute monologue came back with reference labels alternating line by line, which makes the score meaningless. Fewer trustworthy lines beat many uncertain ones.

Current state, two 12-minute episodes of the same two-person podcast:

| file | accuracy | majority baseline |
|---|---|---|
| #367 | **92.1 %** over 38 lines | 52.6 % |
| #73 | **100.0 %** over 33 lines | 63.6 % |
| #72 | 87.9 % over 33 lines | 87.9 % |

All three now yield exactly two clusters for two speakers. #72 stays at its baseline and cannot currently be scored better: its reference lines are short, so only four of them clear the 6 s bar for voice 0 against 29 for voice 1. The full cross-tab shows the split is real there (21 of 27 second-voice lines land in the second cluster) — the *score* is reference-limited, not the pipeline. Quote #367 and #73; treat #72 as unmeasured.

Tried and measured as *not* helping, so do not retry them blind: raising `CLUSTER_DIST` beyond the measured band, tolerating any overlapped speech in anchors, and a Viterbi switch penalty above 0.2. **Do not interpolate `EMBED_LENS`**: 4 s, 6 s and 8 s each separate cleanly, but 3 s and 5 s collapsed every distance toward zero (same ≤0.27 / other ≤0.37) — measure any new length before adding it.
- **A better embedding model would not have helped, and is not the next step.** `wespeaker_en_voxceleb_CAM++` separates two voices cleanly — with equal-length, unspliced excerpts, ≤0.20 within a speaker and ≥0.71 across, measured on this very audio. Every failure so far came from *how* audio was cut for it, not from the model. `pyannote/wespeaker-voxceleb-resnet34-LM` (what pyannote 3.1 uses) is ungated on HF but ships PyTorch only, so adopting it means an ONNX conversion for no demonstrated gain.
- **The sibling `HMS-Transkription-App` does this properly and is the reference to compare against.** It shells out to **WhisperX** (`--diarize`), which brings three things this module does not have: the complete, calibrated `pyannote/speaker-diarization-community-1` pipeline instead of the raw `segmentation-3.0` ONNX model plus a hand-rolled clusterer; **word-level forced alignment** before diarization, so speakers attach to words rather than to Whisper's loose segment timestamps (`speakers_for_lines` maps turns onto whole lines, which is why a short interjection lands on the wrong speaker — on #367 the guest's first three-second greeting at 00:50 still reads as the host, while every long turn is now correct); and licensed access to the gated pyannote weights via an HF token. VoxMD deliberately avoids the token — that is the tradeoff, and the quality gap is its price. Before investing further here, decide whether to close that gap or to accept it.
- Overlapped speech must **not** be used as a clustering anchor, not even a little: tolerating 5 % collapsed a two-speaker file to one cluster. A blended voiceprint sits between the two real ones and bridges them. Few clean anchors beat many contaminated ones — everything else is assigned by nearest centroid afterwards.
- `audio::Resampler` is stateful (biquads, fractional read position). Live capture must reuse **one** instance via `resampler_to_16k` — a fresh one per chunk puts a settling transient at every chunk boundary.
- The dictation tail must still be transcribed after `STOP` is set, so `transcribe_buffer` takes its abort predicate as a parameter rather than reading `STOP` itself.
- `MAX_SPEAKERS_CAP` (`config.rs`) and `MAX_SPEAKERS` (`src/lib/configStore.ts`) are checked against each other by a Rust test, as are the two `AUDIO_EXTENSIONS` lists and `is_local_endpoint` / `isLocalEndpoint`.
- **A local model server needs no API key.** `summary_enabled()` is `include_summary && (key non-empty || endpoint_is_local())`, and `summaryWouldRun` mirrors it in TypeScript. Without this, picking Ollama or LM Studio and leaving the key blank silently skipped the summary. `verify_api_key`, `list_llm_models`, `improve_text` and `translate_text` apply the same exemption.
- Provider presets live in `src/lib/llmProviders.ts` — DeepSeek, OpenRouter, OpenAI, Anthropic, Google Gemini, Mistral, Groq, xAI, Together, Ollama, LM Studio, Custom. Each `baseUrl` was probed for a `/models` route. Anthropic and Gemini are reached through their OpenAI compatibility layers, which is why their paths look unusual. The backend never reads `llm_provider`; it only ever talks to `api_base_url`.
- `cpal` links `libasound.so.2` and `keepawake` links `libdbus-1.so.3`; both are declared in `bundle.linux.deb.depends`.
- `gpu-vulkan` is opt-in; `use_gpu` only applies when the binary was built with that feature **and** the Vulkan loader is present at runtime (`vulkan_runtime::gpu_usable()`). Missing `libvulkan.so` no longer prevents startup (link stub + runtime probe).
- `delete_source_after_success` defaults to **false**. When enabled it deletes **`local_audio` only** (local files and downloaded podcast audio) — **never** the Markdown. Deletion failure is reported as a note, not a hard error.
- `whisper_model` accepts a preset name (`turbo`, `large-v3`, …) **or** a local path ending in `.bin` or `.gguf` (path detection in `config.rs::looks_like_whisper_path`). UI: preset dropdown or **Custom path…** + file picker. Dictation uses `dictation_model` the same way.
- `podcast::download_to_file_blocking` uses `Handle::current().block_on` and must be called from a thread with a Tokio runtime context (true inside `spawn_blocking`).
- The summary is **skipped silently when no API key is set** (`summary_enabled()`), so the app runs fully offline; validation only fails if the transcript is also disabled (empty output).
- Whisper thread count is auto-detected (cores − 1); LLM sampling is fixed in `llm.rs` — neither is a setting anymore. Old stores with `temperature`/`maxTokens`/`whisperThreads` load fine (unknown fields ignored, dropped on next save).
- `ort` must stay at `=2.0.0-rc.10` until `pyannote-rs` pins it; later rcs do not compile against this crate.
- **onnxruntime is not linked in.** `ort` uses `load-dynamic`, and `onnx_runtime.rs` downloads the matching Microsoft release (pinned version + SHA-256 per target) on first diarized run. That took the CPU binary from 39.4 MB to 19.1 MB and the released Vulkan binary from 75.9 MB to 55.6 MB, for a feature that is off by default. `ort::MINOR_VERSION` is checked against the pinned release by a `const` assertion, so an `ort` bump fails the build until the assets and hashes are updated.
- **Never add `alternative-backend` to `ort`.** It looks like the way to stop `download-binaries` (on transitively via `pyannote-rs`, and not switchable off by a dependent) from fetching a 94 MB static archive per clean build. It also replaces `ort::api()`'s `get_or_init(|| dlopen(…))` with a bare `get()`, so `load-dynamic` never initialises and the first `Session` panics with *"attempted to use `ort` APIs before initializing a backend"*. The crate compiles, every unit test passes and CI goes green on both platforms — only real diarization breaks. `onnx_runtime::tests::ort_features_let_load_dynamic_initialise_itself` guards the manifest; `diarize::tests::onnx_session_starts_with_the_shipped_features` opens a real session wherever the assets are cached.
- **Before tagging a release, run `cargo test` on a machine that has run a diarized batch.** The ONNX session test skips where `~/.cache/voxmd/diarize/` is empty, which includes CI — so CI alone cannot tell you diarization still starts.
- After `batch_complete` no queue row may still show an active stage. `settleStrandedRows` (`src/lib/jobs.ts`) sweeps them: a backend error turns them into failures carrying that message, anything else into skips. A panic in the Whisper task is exactly the case the backend's own per-item events cannot cover.
- `ort` **panics** when it cannot open its dylib. Every path into a `Session` must go through `onnx_runtime::init`, which checks the file first and returns a normal error — diarization then degrades to the unlabeled transcript.
- `alternative-backend` (→ `ort-sys/disable-linking`) is on because `download-binaries` still arrives through `pyannote-rs`'s `ort` defaults, which a dependent cannot switch off. Without it every clean build fetches a 94 MB static archive it never links.
- **All models live in ONE directory** — `dirs::data_local_dir()/VoxMD/models` (`%LOCALAPPDATA%\VoxMD\models`, `~/.local/share/VoxMD/models`), overridable with `VOXMD_MODELS_DIR`. Per-user data, not a cache: a cleanup tool evicting a 3 GB Whisper model would be a nasty surprise. Not the program directory either — an installed build cannot write beside its executable, and an AppImage's mount is read-only and temporary. `paths::migrate_legacy_models` moves the 1.0.x `~/.cache/voxmd/{whisper,diarize}` contents across at startup so nobody re-downloads gigabytes.
- `model_download::managed_files` is the explicit list of everything the app downloads, and both `cache_stats` (size for Settings) and `clear_model_cache` work from it. Never "delete everything in the directory": the location is user-overridable, and deleting unknown files out of a directory someone pointed at their own data would be unforgivable.
- **All runtime downloads land in the directory they are loaded from.** Whisper models: `~/.cache/voxmd/whisper/` (`model_download::cache_dir`). Diarization — both ONNX models *and* `libonnxruntime.so` / `onnxruntime.dll` — `~/.cache/voxmd/diarize/` (`diarize::cache_dir`, reached only via `diarize::cached` / `onnx_runtime::library_path`). Tests in both modules pin this.
