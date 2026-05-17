# Contributing

Danke für dein Interesse an VoxMD!

## Entwicklung

1. [Rust (stable)](https://rustup.rs/) und [Node.js LTS](https://nodejs.org/)
2. Systemabhängigkeiten für Tauri v2 (siehe [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/))
3. Optional: Vulkan SDK / Loader für Release-Builds mit GPU (`npm run tauri:vulkan`)

```bash
npm install
npm run tauri dev
```

## Whisper-Modell

Lege eine GGUF-/GGML-Datei (z. B. von Hugging Face) lokal ab und trage den Pfad in den **Einstellungen** ein.

## Pull Requests

- Kleine, fokussierte Änderungen
- `npm run build` und `cargo check` im Ordner `src-tauri` sollten grün sein
- Beschreibe Motivation und ggf. Screenshots

## Community-Verhaltenskodex

Siehe [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).
