# VoxMD

**Local audio transcription with Whisper and Markdown output including LLM post-processing.**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/fly2nbc-oss/VoxMD?label=release)](https://github.com/fly2nbc-oss/VoxMD/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/fly2nbc-oss/VoxMD/ci.yml?label=CI&logo=github)](https://github.com/fly2nbc-oss/VoxMD/actions/workflows/ci.yml)
[![Platforms](https://img.shields.io/badge/ci-Windows%20%7C%20Linux-blue.svg)](https://github.com/fly2nbc-oss/VoxMD/actions/workflows/ci.yml)

VoxMD is a **Tauri v2** desktop application (Rust backend, React/TypeScript frontend). It transcribes local audio files and **podcast episodes (RSS feeds)** locally using **whisper.cpp** (via `whisper-rs`), optionally generates a structured summary via an **OpenAI-compatible API** (e.g. Deepseek), and writes a **Markdown file** per source.

---

## Table of Contents

- [Screenshots](#screenshots)
- [Features](#features)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Supported Platforms & Formats](#supported-platforms--formats)
- [Development & Build](#development--build)
- [Releases](#releases)
- [Troubleshooting](#troubleshooting)
- [Roadmap & Known Issues](#roadmap--known-issues)
- [Contributing](#contributing)
- [License](#license)

---

## Screenshots

![VoxMD main window — Linux, light theme](./screenshots/Linux_light.png)

Dark mode uses the same layout with theme colours from Settings / system preference.

## Features

- **Pipelined processing**: At most **one** Whisper transcription and **one** LLM job run at the same time (bounded queue). While the LLM works on file *n*, Whisper may transcribe file *n+1* — never more than one of each stage.
- **Podcast feeds**: Paste an RSS/Atom feed URL — VoxMD lists all episodes with audio enclosures in the queue. Episodes are downloaded temporarily for transcription and removed afterwards; only the Markdown lands in your chosen output folder.
- **Queue management**: Add audio files via the file picker or **drag & drop** anywhere in the window; select individual entries or all at once (checkbox column) and remove them from the list before starting.
- **Configurable Markdown output**: Toggle the **metadata block** (file / episode info), the **LLM summary**, and the **transcript** independently. With the summary disabled, no API access is needed at all.
- **Progress**: Per-file **Status** badge plus **Details** (download / transcription / summary); footer shows overall queue progress and optional model-download progress.
- **English UI** with light/dark theme (slate-blue accents, Lucide outline icons); **delete-source toggle** directly in the toolbar.
- **Settings**: API URL, key, LLM model, Whisper model name or local GGUF path, transcription language (Whisper), **summary language** (default: system locale; or ISO code) — persisted via `@tauri-apps/plugin-store`. Without an API key, the summary is skipped automatically and VoxMD runs fully offline.
- **Whisper models**: Known names (e.g. `turbo`) download from Hugging Face into `~/.cache/voxmd/whisper/`; dropdown shows size hints and cache status; **Clear cache** removes downloaded models.
- **Audio formats**: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (decoded via Symphonia).
- **Optional Vulkan**: Cargo feature `gpu-vulkan` for GPU-backed Whisper where the system provides Vulkan.

## Quick Start

1. [Download a release](https://github.com/fly2nbc-oss/VoxMD/releases/latest): **Windows** installers (`.msi`, NSIS setup `.exe`) and optional **portable** `VoxMD.exe` — no installer; [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) must be present on the PC. **Linux**: `.deb` / `.rpm`; `.AppImage` when bundled successfully.
2. Launch the app — the default Whisper model (`turbo`, ~800 MB) is **downloaded automatically** when needed (unless you point to a local GGUF path).
3. Enter your **API key** and **base URL** (e.g. `https://api.deepseek.com`) in settings and press **Save**. No key? The summary is skipped automatically and only the transcript is written.
4. Add **Files** (local audio) or a **Podcast** feed and press **Start**.

Output: a `.md` file next to each local audio file; podcast episodes write into the output folder chosen in the Podcast dialog. Existing `.md` files are skipped, so re-running a batch is idempotent.

## Usage

### Settings (gear icon)

| Field | Description | Default |
|---|---|---|
| Markdown output | Toggle **Metadata block**, **Summary (LLM)**, and **Transcript** independently | all ✅ |
| API Base URL / Key / Model | OpenAI-compatible endpoint for the summary; greyed out while the summary is off. **No key → summary is skipped automatically.** | `https://api.deepseek.com` / *(empty)* / `deepseek-v4-pro` |
| Whisper model | Preset name (`turbo`, …) or absolute path to a `.gguf` file | `turbo` |
| Transcription language | ISO 639-1 code for Whisper (e.g. `de`) | `de` |
| Summary language | `system` (OS locale) or ISO 639-1 for LLM summary headings and text | `system` |

LLM sampling is fixed internally (temperature 0.3, generous token limit) and Whisper thread count is auto-detected — these are no longer settings.

**Save** shows a brief confirmation and closes the panel. The **delete-source toggle** (trash icon) lives in the toolbar: when active (red), local source audio is deleted after a successful `.md` write; podcast downloads are always temporary regardless of this toggle.

### Model selection

Pick a preset in the dropdown (sizes shown). A ✓ means the GGUF is already cached. Presets without ✓ download before transcription. **Clear cache** deletes files under `~/.cache/voxmd/whisper/`.

### Output Markdown layout

```markdown
# Title (from audio tags or episode title)

- Podcast: Feed title          ← metadata block (optional)
- Episode: Episode title
- Published: 2026-06-01
- Link: https://…

## Summary in One Sentence     ← LLM summary sections (optional)
…

## Transcript                  ← raw Whisper transcript (optional)

[HH:MM:SS] Utterance text.
```

Each section is controlled by the **Markdown output** checkboxes in Settings. Transcript lines are the raw Whisper output with `[HH:MM:SS]` timestamps; the summary quotes reference those timestamps.

## Supported Platforms & Formats

| Platform | CI / release binaries | Local build |
|----------|------------------------|-------------|
| Linux    | ✅ tested on Ubuntu runner | ✅ |
| Windows  | ✅ | ✅ |

Audio formats: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS.

## Development & Build

Prerequisites: **Rust stable**, **Node LTS**, system packages for [Tauri v2](https://v2.tauri.app/start/prerequisites/).

**Linux additionally requires:**

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf clang libclang-dev llvm-dev
```

```bash
git clone https://github.com/fly2nbc-oss/VoxMD.git
cd VoxMD
npm install
npm run tauri dev
```

**Production build (CPU, default feature set):**

```bash
npm run tauri build
```

**With Vulkan / GPU Whisper** (Vulkan SDK or headers required on the build machine). The normal `tauri build` is CPU-only; use this command when you want GPU-backed Whisper:

```bash
npm run tauri:vulkan
```

## Releases

Tags matching `v*` trigger [.github/workflows/tauri-release.yml](.github/workflows/tauri-release.yml), which builds **Linux** and **Windows** packages and attaches checksum files:

| Artifact | Description |
|----------|-------------|
| `SHA256SUMS-linux.txt` | Hashes for `.deb`, `.rpm`, `.AppImage` (if produced) |
| `SHA256SUMS-windows.txt` | Hashes for `.msi` / installer outputs |

## Troubleshooting

- **Summary missing from the output** — no API key configured: the summary is skipped automatically. Enter a key in Settings to enable it.
- **Feed loads no episodes** — VoxMD only lists entries with an audio enclosure (`<enclosure>` / media content with an `audio/*` MIME type or audio file extension).
- **A file is skipped with "exists"** — the target `.md` already exists; delete or rename it to re-process.
- **GPU checkbox greyed out** — this binary was built without the `gpu-vulkan` feature; use a Vulkan release build or `npm run tauri:vulkan`.
- **Linux AppImage fails silently** — bundling requires `linuxdeploy`; `.deb`/`.rpm` packages are unaffected.

## Roadmap & Known Issues

- Whisper does not expose fine-grained percentage progress to the UI; stages **Download** / **Whisper** / **LLM** still indicate where time is spent.
- No hard cancellation of an in-flight job (status updates until the pipeline finishes); episode downloads finish before a cancel takes effect.
- Linux AppImage bundling may fail if `linuxdeploy` is missing on the runner or developer machine.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE).
