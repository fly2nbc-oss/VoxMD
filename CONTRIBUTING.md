# Contributing

Thank you for your interest in VoxMD!

## Prerequisites

1. [Rust (stable)](https://rustup.rs/) and [Node.js LTS](https://nodejs.org/)
2. System dependencies for [Tauri v2](https://v2.tauri.app/start/prerequisites/)
3. **Linux additionally requires**: `clang`, `libclang-dev`, `llvm-dev` (for `whisper-rs` bindgen)

```bash
# Debian/Ubuntu
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf clang libclang-dev llvm-dev
```

## Getting Started

```bash
git clone https://github.com/fly2nbc-oss/VoxMD.git
cd VoxMD
npm install
npm run tauri dev
```

## Checking Your Build

```bash
npm run build               # Frontend (TypeScript + Vite)
cargo check --manifest-path src-tauri/Cargo.toml   # Rust library/binary
```

Optional full desktop bundle:

```bash
npm run tauri build
```

Vulkan-enabled desktop bundle:

```bash
npm run tauri:vulkan
```

Linux AppImage bundling requires `linuxdeploy` where enabled in `tauri.conf.json`; `.deb`/`.rpm` may still succeed without it.

## Continuous Integration

`.github/workflows/ci.yml` runs on pushes and PRs targeting **`main`** for:

- **Ubuntu 22.04** — full Tauri build with `gpu-vulkan`
- **Windows** — full Tauri build with `gpu-vulkan`

Tagged releases (`v*`) use `.github/workflows/tauri-release.yml` with the **same matrix** (Linux + Windows). Published checksum files are named `SHA256SUMS-linux.txt` and `SHA256SUMS-windows.txt`.

## Pull Requests

- Keep changes small and focused.
- Describe motivation and — for UI changes — attach screenshots (light/dark if relevant).
- `npm run build` and `cargo check --manifest-path src-tauri/Cargo.toml` must pass.

## Whisper Models

Either configure a **preset name** (downloaded automatically on demand) or an absolute path to a local `.gguf` file in Settings.

## LLM Transcript Stage

Speaker tagging splits the raw Whisper text into chunks (`transcript_chunk_chars`). Invalid chunk outputs trigger one automatic repair attempt before the job fails — if you adjust prompts in `src-tauri/src/llm.rs`, keep the `[HH:MM:SS] **Label:**` contract in mind.

## Code of Conduct

See [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).
