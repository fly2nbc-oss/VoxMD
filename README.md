# VoxMD

**Lokale Audio-Transkription mit Whisper und Markdown-Ausgabe inkl. LLM-Nachbearbeitung.**

![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)

VoxMD ist eine **Tauri v2**-Desktop-Anwendung (Rust-Backend, React/TypeScript-Frontend). Sie transkribiert Audiodateien lokal mit **whisper.cpp** (via `whisper-rs`), reichert das Ergebnis per **OpenAI-kompatibler API** (z. B. Deepseek) mit Sprecher-Zuordnung und Zusammenfassung an und schreibt eine **Markdown-Datei** pro Quelle.

## Screenshots

_Placeholder: Lege Demo-Bilder unter `./screenshots/` ab (Hell- und Dunkelmodus, Windows/Linux/macOS) und binde sie hier ein._

## Features

- **Parallele Pipeline**: Während die LLM-Phase für Datei *n* läuft, transkribiert Whisper bereits Datei *n+1*.
- **Fortschritt**: Pro-Datei-Status und Gesamtfortschritt über Tauri-Events.
- **Design**: UI-Design-System (Hell/Dunkel, Slate-Blau, Lucide Outline-Icons).
- **Konfiguration**: API-URL, Key, Modell, Chunk-Größe, Whisper-Modellpfad, optional GPU – persistent im Store.
- **Formate**: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (Dekodierung über Symphonia).
- **Optional Vulkan**: Cargo-Feature `gpu-vulkan` für GPU-Beschleunigung (System muss Vulkan bereitstellen).

## Quick Start

1. [Release-Download](https://github.com/fly2nbc-oss/VoxMD/releases) *(nach erstem Release)* oder lokal bauen (siehe unten).
2. Whisper **GGUF/GGML-Modell** beschaffen und in den **Einstellungen** den absoluten Pfad setzen.
3. **API-Key** und **Base-URL** (z. B. Deepseek) eintragen und **Speichern**.
4. **Ordner** oder **Dateien** wählen und **Start** drücken.

Ausgabe: `.md` neben der Audiodatei (bzw. im gleichen Ordner) – analog zu klassischen Transkript-Pipelines.

## Unterstützte Plattformen & Formate

| Plattform | Status        |
|-----------|---------------|
| Linux     | unterstützt   |
| Windows   | unterstützt   |
| macOS     | unterstützt   |

Audio: siehe Liste in den Quellen (`meta.rs` / UI-Filter).

## Entwicklung & Build

Voraussetzungen: **Rust stable**, **Node LTS**, System-Pakete für [Tauri v2](https://v2.tauri.app/start/prerequisites/).

```bash
git clone https://github.com/fly2nbc-oss/VoxMD.git
cd VoxMD
npm install
npm run tauri dev
```

**Produktions-Build (CPU, Standard):**

```bash
npm run tauri build
```

**Mit Vulkan/GPU** (Vulkan-Development-Packages/SDK auf dem Build-Rechner):

```bash
npm run tauri:vulkan
```

## Releases

Für gebündelte Artefakte (`.msi`, `.dmg`, `.AppImage`, `.deb`, …) siehe GitHub **Actions** und **Releases**. Bei Tags `v*` baut der Workflow `.github/workflows/tauri-release.yml` mit `tauri-action` (setze Upload-Token/Secrets nach Bedarf).

## Roadmap & Known Issues

- Fein-granularer Whisper-Fortschritt (C++-Callback) ist derzeit nicht angebunden; Stufen **Whisper** / **LLM** und LLM-Chunk-Zähler sind aktiv.
- Abbruch laufender Jobs: aktuell ohne harten Cancel (UI zeigt Status bis zum Ende der Pipeline).

## Contributing

Siehe [CONTRIBUTING.md](./CONTRIBUTING.md).

## Lizenz

Apache-2.0 – siehe [LICENSE](./LICENSE).
