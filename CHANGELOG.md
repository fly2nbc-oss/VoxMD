# Changelog

## [0.1.0] - 2026-05-17

### Added

- Erste Tauri-Desktop-App (Rust + React): Whisper (lokal) + OpenAI-kompatible LLM-API
- Pipeline: parallele Phasen Whisper → LLM (nächste Datei transcribieren während LLM läuft)
- Fortschritt über Tauri-Events (`job_progress`, `batch_complete`)
- UI nach Design-System v1.2 (Hell/Dunkel, Lucide-Icons)
- Einstellungen mit `@tauri-apps/plugin-store`
- Optional GPU-Vulkan über Cargo-Feature `gpu-vulkan`
- GitHub-Actions-Workflow für Releases
