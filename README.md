# VoxMD

**Lokale Audio-Transkription mit Whisper und Markdown-Ausgabe inkl. LLM-Nachbearbeitung.**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/fly2nbc-oss/VoxMD?label=release)](https://github.com/fly2nbc-oss/VoxMD/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/fly2nbc-oss/VoxMD/tauri-release.yml?label=build)](https://github.com/fly2nbc-oss/VoxMD/actions/workflows/tauri-release.yml)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#unterstützte-plattformen--formate)

VoxMD ist eine **Tauri v2**-Desktop-Anwendung (Rust-Backend, React/TypeScript-Frontend). Sie transkribiert Audiodateien lokal mit **whisper.cpp** (via `whisper-rs`), reichert das Ergebnis per **OpenAI-kompatibler API** (z. B. Deepseek) mit Sprecher-Zuordnung und Zusammenfassung an und schreibt eine **Markdown-Datei** pro Quelle.

---

## Inhalt

- [Screenshots](#screenshots)
- [Features](#features)
- [Quick Start](#quick-start)
- [Nutzung](#nutzung)
- [Unterstützte Plattformen & Formate](#unterstützte-plattformen--formate)
- [Entwicklung & Build](#entwicklung--build)
- [Releases](#releases)
- [Roadmap & Known Issues](#roadmap--known-issues)
- [Contributing](#contributing)
- [Lizenz](#lizenz)

---

## Screenshots

_Placeholder: Lege Demo-Bilder unter `./screenshots/` ab (Hell- und Dunkelmodus) und binde sie hier ein._

## Features

- **Parallele Pipeline**: Während die LLM-Phase für Datei *n* läuft, transkribiert Whisper bereits Datei *n+1*.
- **Fortschritt**: Pro-Datei-Status und Gesamtfortschritt über Tauri-Events.
- **Design**: UI-Design-System (Hell/Dunkel, Slate-Blau, Lucide Outline-Icons).
- **Konfiguration**: API-URL, Key, Modell, Chunk-Größe, Whisper-Modellpfad, optional GPU – persistent im Store.
- **Formate**: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (Dekodierung über Symphonia).
- **Optional Vulkan**: Cargo-Feature `gpu-vulkan` für GPU-Beschleunigung (System muss Vulkan bereitstellen).

## Quick Start

1. [Release-Download](https://github.com/fly2nbc-oss/VoxMD/releases) *(nach erstem Release)* oder lokal bauen (siehe unten).
2. App starten – das Whisper-Modell (`turbo`, ~800 MB) wird beim ersten Start **automatisch** aus HuggingFace heruntergeladen.
3. **API-Key** und **Base-URL** (z. B. `https://api.deepseek.com`) in den Einstellungen eintragen und **Save** drücken.
4. **Folder** oder **Files** wählen und **Start** drücken.

Ausgabe: `.md` neben der Audiodatei (bzw. im gleichen Ordner).

## Nutzung

### Einstellungen (Zahnrad-Icon)

| Feld | Beschreibung | Standard |
|---|---|---|
| API Base URL | OpenAI-kompatibler Endpunkt | `https://api.deepseek.com` |
| API Key | Dein API-Schlüssel | *(leer)* |
| Model | LLM-Modellname | `deepseek-v4-pro` |
| Temperature | Kreativität des LLM (0–2) | `0.7` |
| Max Tokens | Maximale Antwortlänge | `65536` |
| Transcript chunk chars | Zeichen pro LLM-Chunk | `32768` |
| Whisper model | Modellname (`turbo`, `large-v3`, …) oder lokaler Pfad | `turbo` |
| Delete source after success | Quelldatei nach Erfolg löschen | ✅ |
| Whisper verbose output | Debug-Ausgabe von whisper.cpp | ☐ |

### Modell-Auswahl

Im Einstellungs-Dialog kann ein Whisper-Modell aus dem Dropdown ausgewählt werden. Modelle ohne ✓ werden beim nächsten Start automatisch heruntergeladen. Mit **Clear cache** werden alle gecachten Modelle gelöscht (`~/.cache/voxmd/whisper/`).

### Ausgabe-Format

```
# Titel

[KI-Zusammenfassung]

## Originaltranskript

**Sprecher A**: …  
**Sprecher B**: …
```

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
