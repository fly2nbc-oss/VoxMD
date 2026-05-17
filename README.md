# VoxMD

**Local audio transcription with Whisper and Markdown output including LLM post-processing.**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/fly2nbc-oss/VoxMD?label=release)](https://github.com/fly2nbc-oss/VoxMD/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/fly2nbc-oss/VoxMD/ci.yml?label=CI&logo=github)](https://github.com/fly2nbc-oss/VoxMD/actions/workflows/ci.yml)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#supported-platforms--formats)

VoxMD is a **Tauri v2** desktop application (Rust backend, React/TypeScript frontend). It transcribes audio files locally using **whisper.cpp** (via `whisper-rs`), enriches the result with speaker identification and a summary via an **OpenAI-compatible API** (e.g. Deepseek), and writes a **Markdown file** per source.

---

## Table of Contents

- [Screenshots](#screenshots)
- [Features](#features)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Supported Platforms & Formats](#supported-platforms--formats)
- [Development & Build](#development--build)
- [Releases](#releases)
- [Roadmap & Known Issues](#roadmap--known-issues)
- [Contributing](#contributing)
- [License](#license)

---

## Screenshots

<!-- Add demo images to ./screenshots/ (light and dark mode, Windows + Linux + macOS) -->

| Main window (light) | Main window (dark) |
|---|---|
| ![Light](./screenshots/main-light.png) | ![Dark](./screenshots/main-dark.png) |

## Features

- **Parallel pipeline**: While the LLM phase processes file *n*, Whisper is already transcribing file *n+1*.
- **Progress tracking**: Per-file status and overall progress via Tauri events.
- **Design**: UI design system (light/dark, slate-blue, Lucide outline icons).
- **Configuration**: API URL, key, model, chunk size, Whisper model path, optional GPU — persisted in store.
- **Formats**: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (decoded via Symphonia).
- **Optional Vulkan**: Cargo feature `gpu-vulkan` for GPU acceleration (system must provide Vulkan).

## Quick Start

1. [Download a release](https://github.com/fly2nbc-oss/VoxMD/releases/latest) (`.msi` / `.dmg` / `.AppImage` / `.deb`) or build locally (see below).
2. Launch the app — the Whisper model (`turbo`, ~800 MB) is **automatically downloaded** from HuggingFace on first start.
3. Enter your **API key** and **base URL** (e.g. `https://api.deepseek.com`) in settings and press **Save**.
4. Select a **Folder** or **Files** and press **Start**.

Output: a `.md` file next to each audio file (in the same folder).

## Usage

### Settings (gear icon)

| Field | Description | Default |
|---|---|---|
| API Base URL | OpenAI-compatible endpoint | `https://api.deepseek.com` |
| API Key | Your API key | *(empty)* |
| Model | LLM model name | `deepseek-v4-pro` |
| Temperature | LLM creativity (0–2) | `0.7` |
| Max Tokens | Maximum response length | `65536` |
| Transcript chunk chars | Characters per LLM chunk | `32768` |
| Whisper model | Model name (`turbo`, `large-v3`, …) or local path | `turbo` |
| Delete source after success | Delete source file after successful processing | ✅ |
| Whisper verbose output | Debug output from whisper.cpp | ☐ |

### Model Selection

In the settings dialog you can select a Whisper model from the dropdown. Models without ✓ are automatically downloaded on next start. Use **Clear cache** to delete all cached models (`~/.cache/voxmd/whisper/`).

### Output Format

```
# Title

[AI summary]

## Original Transcript

**Speaker A**: …
**Speaker B**: …
```

## Supported Platforms & Formats

| Platform | Status      |
|----------|-------------|
| Linux    | supported   |
| Windows  | supported   |
| macOS    | supported   |

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

**Production build (CPU, standard):**

```bash
npm run tauri build
```

**With Vulkan/GPU** (Vulkan development packages/SDK required on the build machine):

```bash
npm run tauri:vulkan
```

## Releases

Every release (`v*` tag) automatically builds packages for all platforms and publishes them under [Releases](https://github.com/fly2nbc-oss/VoxMD/releases):

| Platform | Asset |
|----------|-------|
| Windows  | `.msi` installer |
| macOS    | `.dmg` |
| Linux    | `.AppImage`, `.deb`, `.rpm` |

Each release also includes `SHA256SUMS.txt` for integrity verification.

## Roadmap & Known Issues

- Fine-grained Whisper progress (C++ callback) is not yet wired up; **Whisper** / **LLM** stage indicators and LLM chunk counters are active.
- Job cancellation: currently no hard cancel (UI shows status until the pipeline finishes).

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

Apache-2.0 — see [LICENSE](./LICENSE).
