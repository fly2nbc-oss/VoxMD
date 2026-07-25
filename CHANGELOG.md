# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

## [0.10.0] - 2026-06-12

### Added

- **Podcast feeds**: new toolbar button loads an RSS/Atom feed (`feed-rs`) and queues all episodes with audio enclosures. Episodes are downloaded lazily into a self-deleting temp file during the Whisper stage; the Markdown lands in a per-feed output folder chosen in the dialog (last folder remembered).
- **Queue selection**: checkbox column with select-all and a **Remove** toolbar button to drop marked entries from the list.
- **Markdown output toggles** in Settings: metadata block (file / episode info), LLM summary, and transcript can be enabled independently (at least one of summary/transcript required). With the summary disabled, no API key is needed.
- **Drag & drop**: audio files dropped anywhere in the window are added to the queue (same formats as the file picker; unsupported items are ignored with a note).
- New job stage **Download** with percentage progress for podcast episodes.
- `SECURITY.md`, Dependabot config (GitHub Actions, Cargo, npm — weekly, grouped).

### Changed

- **Summary requires an API key**: without one it is skipped silently instead of failing the run (`AppConfig::summary_enabled`); the API fields in Settings are greyed out while the summary is disabled.
- **Delete source after success** moved from Settings into the toolbar as a trash-icon toggle (persists immediately; red = active). It only applies to local files — episode downloads are always temporary.
- Settings **Save** now shows a confirmation state and closes the panel; errors keep it open.
- **Files** and **Podcast** now append to the queue (deduplicated) instead of replacing it.
- Summary prompt rewritten: no speaker labels, quotes cite `[HH:MM:SS]` timestamps, sections are omitted when empty, episode metadata is passed as context, fixed sampling (temperature 0.3).
- Transcript section in the output is the raw Whisper text under `## Transcript` (was the LLM-labelled text under `## Original Transcript`).
- Markdown filenames keep dots in titles (`Path::with_extension` no longer truncates e.g. `Ep. 5 - …`).
- CI: new lint/test gate job (tsc, `cargo fmt`, `clippy -D warnings`, `cargo test`) before the build matrix; concurrency groups cancel superseded runs.

### Removed

- **Speaker detection / labelling** (LLM transcript pass, chunking, format validation, repair pass) — the LLM is now only used for the summary.
- **Transcript chunk size** setting (`transcriptChunkChars`) — obsolete without the transcript LLM pass.
- **Temperature**, **Max tokens**, and **Whisper CPU threads** settings — the summary uses fixed sampling (0.3 / 8192) and Whisper auto-detects threads (cores − 1). Old stored values are dropped on load.
- **Folder** toolbar button and the `collect_audio_in_directory` command (use multi-select in the file picker; `walkdir` dependency dropped).

## [0.9.8] - 2026-05-25

### Fixed

- Config validation: API URL scheme, numeric bounds, local Whisper paths (`.bin`/`.gguf`), non-UTF-8 model path rejection.
- Pipeline: surface source-file deletion failures in job status; reject non-UTF-8 Whisper paths explicitly.
- Frontend: clearer error messages (`toMsg`), guarded numeric settings inputs, warn when model list unavailable.
- Directory scan: canonicalize folder paths before walking.

### Changed

- Remove unused Vite/Tauri template assets in `public/`.

### Removed

- Default `public/` SVG favicons (app icons remain under `src-tauri/icons/`).

## [0.9.7] - 2026-05-24

### Changed

- Repository is **public**; README dynamic release/CI badges work via shields.io.
- Release workflow publishes GitHub releases immediately (`releaseDraft: false`).
- CI: Windows `rust-cache` post-step no longer fails (`cache-targets: false`, `CARGO_TARGET_DIR` set before cache).

### Removed

- **Whisper verbose output** setting.
- `VoxMD.jpg` and `.cursor/plans/` from version control; expanded `.gitignore` for secrets, builds, and local IDE data.

## [0.9.6] - 2026-05-24

### Added

- Settings: **Summary language** — default uses the system locale (`system`); optional fixed ISO 639-1 code (independent of Whisper transcription language).

### Changed

- Windows release: portable executable is published as **`VoxMD.exe`** (no versioned portable filename).

## [0.9.0] - 2026-05-17

### Added

- Tauri v2 desktop shell (Rust backend, React + TypeScript + Vite frontend).
- Local transcription via **whisper.cpp** through `whisper-rs` (no cloud ASR).
- LLM post-processing over an OpenAI-compatible HTTP API: speaker-labelled transcript plus structured summary (`async-openai`).
- Bounded pipeline: concurrent **Whisper** and **LLM** stages with capacity **one** each (`tokio::sync::mpsc` channel buffer = 1).
- Progress events: `job_progress`, `batch_complete`, and `model_download_progress` for Hugging Face GGUF downloads.
- Automatic Whisper GGUF resolution: preset names download into `~/.cache/voxmd/whisper/`; settings dropdown lists presets, sizes, cache markers, and **Clear cache**.
- Supported containers/codecs: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (Symphonia decode).
- Persistent configuration via `@tauri-apps/plugin-store` (API URL/key/model, chunk sizes, Whisper options, delete-after-success).
- Optional GPU inference path via Cargo feature **`gpu-vulkan`** (Vulkan toolchain/SDK required at link time).
- GitHub Actions **CI** workflow (`ci.yml`) on `main` / pull requests (Linux + Windows builds).
- GitHub Actions **release** workflow (`tauri-release.yml`) on `v*` tags (Linux + Windows artifacts, per-platform SHA256 checksum files).

### Changed

- English UI copy; unified compact **app bar**; job table columns **File**, **Status**, **Details** (blue badge styling for LLM stage).
- Default configuration aligned with external scripting conventions (Deepseek defaults, `turbo` Whisper preset, delete-after-success enabled).
- README / docs describe CI scope (Linux and Windows builds).

### Fixed

- LLM transcript chunks: split raw text on Whisper timestamp lines when possible; carry trailing labelled lines into the next chunk for speaker consistency; validate `[HH:MM:SS] **Label:**` lines and monotonic timestamps per chunk; automatic single retry with repair prompt before failing the job.
- Packaging hygiene: RGBA icons for Tauri bundle; CI and releases target **Linux and Windows**.
