# Changelog

Alle wesentlichen Änderungen werden hier dokumentiert.  
Format basiert auf [Keep a Changelog](https://keepachangelog.com/de/1.0.0/), Versionierung folgt [Semantic Versioning](https://semver.org/lang/de/).

---

## [0.9.0] - 2026-05-17

### Hinzugefügt

- Erste Tauri v2 Desktop-App (Rust-Backend, React/TypeScript-Frontend)
- Lokale Transkription mit **whisper.cpp** via `whisper-rs` (kein Cloud-Zwang)
- Parallele Pipeline: Whisper transkribiert Datei *n+1* während LLM Datei *n* verarbeitet
- LLM-Nachbearbeitung: Sprecher-Zuordnung und Zusammenfassung via OpenAI-kompatibler API (z. B. Deepseek)
- Fortschritt-Tracking über Tauri-Events (`job_progress`, `batch_complete`)
- Unterstützte Formate: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS
- Hell/Dunkel-Modus mit Slate-Blau Design-System und Lucide Outline-Icons
- Persistente Einstellungen mit `@tauri-apps/plugin-store`
- Automatischer Whisper-Modell-Download (`turbo` ~800 MB) beim ersten Start
- Optionale GPU-Beschleunigung über Cargo-Feature `gpu-vulkan` (Vulkan Loader erforderlich)
- GitHub Actions Workflow für automatische Multi-Plattform Releases
- CI-Workflow für Build-Validierung bei jedem Push auf `main`
