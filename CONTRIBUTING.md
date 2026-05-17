# Contributing

Danke für dein Interesse an VoxMD!

## Voraussetzungen

1. [Rust (stable)](https://rustup.rs/) und [Node.js LTS](https://nodejs.org/)
2. Systemabhängigkeiten für [Tauri v2](https://v2.tauri.app/start/prerequisites/)
3. **Linux zusätzlich**: `clang`, `libclang-dev`, `llvm-dev` (für `whisper-rs` Bindgen)

```bash
# Debian/Ubuntu
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf clang libclang-dev llvm-dev
```

## Entwicklung starten

```bash
git clone https://github.com/fly2nbc-oss/VoxMD.git
cd VoxMD
npm install
npm run tauri dev
```

## Build prüfen

```bash
npm run build          # Frontend (TypeScript + Vite)
cd src-tauri && cargo check   # Rust
```

## Pull Requests

- Kleine, fokussierte Änderungen bevorzugt
- Beschreibe Motivation und — falls UI-Änderung — Screenshots
- `npm run build` und `cargo check` sollten ohne Fehler durchlaufen

## Whisper-Modell lokal

Lege eine GGUF-/GGML-Datei (z. B. von Hugging Face) ab und trage den Pfad in den **Einstellungen** ein. Alternativ lädt die App `turbo` beim ersten Start automatisch herunter.

## Community-Verhaltenskodex

Siehe [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).
