# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

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
