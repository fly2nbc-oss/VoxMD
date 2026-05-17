# VoxMD

**Local audio transcription with Whisper and Markdown output including LLM post-processing.**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Latest Release](https://img.shields.io/github/v/release/fly2nbc-oss/VoxMD?label=release)](https://github.com/fly2nbc-oss/VoxMD/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/fly2nbc-oss/VoxMD/ci.yml?label=CI&logo=github)](https://github.com/fly2nbc-oss/VoxMD/actions/workflows/ci.yml)
[![Platforms](https://img.shields.io/badge/ci-Windows%20%7C%20Linux-blue.svg)](https://github.com/fly2nbc-oss/VoxMD/actions/workflows/ci.yml)

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

<!-- Place demos under ./screenshots/ (light + dark; Windows/Linux match CI artifacts.) -->

| Main window (light) | Main window (dark) |
|---|---|
| ![Light](./screenshots/main-light.png) | ![Dark](./screenshots/main-dark.png) |

## Features

- **Pipelined processing**: At most **one** Whisper transcription and **one** LLM job run at the same time (bounded queue). While the LLM works on file *n*, Whisper may transcribe file *n+1* — never more than one of each stage.
- **Progress**: Per-file **Status** badge plus **Details** (timestamps / LLM chunk progress); footer shows overall queue progress and optional model-download progress.
- **English UI** with light/dark theme (slate-blue accents, Lucide outline icons).
- **Settings**: API URL, key, LLM model, temperature, max tokens, transcript chunk size, Whisper model name or local GGUF path, optional Whisper verbose logging, delete-after-success — persisted via `@tauri-apps/plugin-store`.
- **Whisper models**: Known names (e.g. `turbo`) download from Hugging Face into `~/.cache/voxmd/whisper/`; dropdown shows size hints and cache status; **Clear cache** removes downloaded models.
- **Audio formats**: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (decoded via Symphonia).
- **Optional Vulkan**: Cargo feature `gpu-vulkan` for GPU-backed Whisper where the system provides Vulkan.

## Quick Start

1. [Download a release](https://github.com/fly2nbc-oss/VoxMD/releases/latest): **Windows** (`.msi`) and **Linux** (`.deb` / `.rpm`; `.AppImage` when bundled successfully).  
   **macOS:** build locally ([Development & Build](#development--build)); CI does not publish `.dmg` binaries.
2. Launch the app — the default Whisper model (`turbo`, ~800 MB) is **downloaded automatically** when needed (unless you point to a local GGUF path).
3. Enter your **API key** and **base URL** (e.g. `https://api.deepseek.com`) in settings and press **Save**.
4. Select **Folder** or **Files** and press **Start**.

Output: a `.md` file next to each audio file (same folder).

## Usage

### Settings (gear icon)

| Field | Description | Default |
|---|---|---|
| API Base URL | OpenAI-compatible endpoint | `https://api.deepseek.com` |
| API Key | Your API key | *(empty)* |
| Model | LLM model name | `deepseek-v4-pro` |
| Temperature | LLM sampling temperature (0–2) | `0.7` |
| Max Tokens | Maximum completion tokens | `65536` |
| Transcript chunk chars | Max characters per raw-transcript slice sent to the LLM | `32768` |
| Whisper model | Preset name (`turbo`, …) or absolute path to a `.gguf` file | `turbo` |
| Delete source after success | Remove source audio after a successful `.md` write | ✅ |
| Whisper verbose output | Forward whisper.cpp debug/progress to the terminal | ☐ |

### Model selection

Pick a preset in the dropdown (sizes shown). A ✓ means the GGUF is already cached. Presets without ✓ download before transcription. **Clear cache** deletes files under `~/.cache/voxmd/whisper/`.

### Output Markdown layout

```markdown
# Title from metadata

## Metadata
… LLM-generated summary sections …

## Original Transcript

[HH:MM:SS] **Speaker label:** Utterance text.
```

Speaker lines follow the strict `[HH:MM:SS] **Label:** …` format produced by the LLM pass.

## Supported Platforms & Formats

| Platform | CI / release binaries | Local build |
|----------|------------------------|-------------|
| Linux    | ✅ tested on Ubuntu runner | ✅ |
| Windows  | ✅ | ✅ |
| macOS    | ❌ not built in GitHub Actions | ✅ (`npm run tauri build` on a Mac) |

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

**With Vulkan / GPU Whisper** (Vulkan SDK or headers required on the build machine):

```bash
npm run tauri:vulkan
```

macOS deployments should keep **macOS 10.15+** as minimum when linking whisper.cpp / GGML (`src-tauri/tauri.conf.json` / `.cargo/config.toml`).

## Releases

Tags matching `v*` trigger [.github/workflows/tauri-release.yml](.github/workflows/tauri-release.yml), which builds **Linux** and **Windows** packages and attaches checksum files:

| Artifact | Description |
|----------|-------------|
| `SHA256SUMS-linux.txt` | Hashes for `.deb`, `.rpm`, `.AppImage` (if produced) |
| `SHA256SUMS-windows.txt` | Hashes for `.msi` / installer outputs |

macOS `.dmg` bundles are **not** produced by this workflow.

## Roadmap & Known Issues

- Whisper does not expose fine-grained percentage progress to the UI; stages **Whisper** / **LLM** and LLM chunk indexing still indicate where time is spent.
- No hard cancellation of an in-flight job (status updates until the pipeline finishes).
- Linux AppImage bundling may fail if `linuxdeploy` is missing on the runner or developer machine.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

Apache-2.0 — see [LICENSE](./LICENSE).
