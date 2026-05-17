# Contributing

Thank you for your interest in VoxMD!

## Prerequisites

1. [Rust (stable)](https://rustup.rs/) and [Node.js LTS](https://nodejs.org/)
2. System dependencies for [Tauri v2](https://v2.tauri.app/start/prerequisites/)
3. **Linux additionally requires**: `clang`, `libclang-dev`, `llvm-dev` (for `whisper-rs` Bindgen)

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
cd src-tauri && cargo check # Rust
```

## Pull Requests

- Keep changes small and focused
- Describe the motivation and — for UI changes — include screenshots
- `npm run build` and `cargo check` should pass without errors

## Whisper Model (local)

Place a GGUF/GGML file (e.g. from Hugging Face) locally and enter the path in **Settings**. Alternatively, the app automatically downloads `turbo` on first start.

## Code of Conduct

See [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).
