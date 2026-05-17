# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning follows [Semantic Versioning](https://semver.org/).

---

## [0.9.0] - 2026-05-17

### Added

- First Tauri v2 desktop app (Rust backend, React/TypeScript frontend)
- Local transcription with **whisper.cpp** via `whisper-rs` (no cloud dependency)
- Parallel pipeline: Whisper transcribes file *n+1* while LLM processes file *n*
- LLM post-processing: speaker identification and summary via OpenAI-compatible API (e.g. Deepseek)
- Progress tracking via Tauri events (`job_progress`, `batch_complete`)
- Supported formats: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS
- Light/dark mode with slate-blue design system and Lucide outline icons
- Persistent settings with `@tauri-apps/plugin-store`
- Automatic Whisper model download (`turbo` ~800 MB) on first start
- Optional GPU acceleration via Cargo feature `gpu-vulkan` (Vulkan loader required)
- GitHub Actions workflow for automatic multi-platform releases
- CI workflow for build validation on every push to `main`
