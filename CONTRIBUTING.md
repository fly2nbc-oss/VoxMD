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
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
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

`.github/workflows/ci.yml` runs on pushes and PRs targeting **`main`**:

- **Lint & Test (Linux)** — `npm run build` (tsc), `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`; gates the build matrix
- **Ubuntu 24.04** — full Tauri build with `gpu-vulkan`
- **Windows** — full Tauri build with `gpu-vulkan`

Superseded runs on the same branch/PR are cancelled automatically. Dependabot keeps GitHub Actions, Cargo, and npm dependencies current (weekly, grouped).

Tagged releases (`v*`) use `.github/workflows/tauri-release.yml` with the **same matrix** (Linux + Windows). Published checksum files are named `SHA256SUMS-linux.txt` and `SHA256SUMS-windows.txt`.

## Pull Requests

- Keep changes small and focused.
- Describe motivation and — for UI changes — attach screenshots (light/dark if relevant).
- `npm run build`, `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` must pass (the CI check job runs exactly these).

## Whisper Models

Either configure a **preset name** (downloaded automatically on demand) or an absolute path to a local `.gguf` file in Settings.

## LLM Summary Stage

The transcript in the output is the **raw Whisper text** (`[HH:MM:SS] text` lines) — there is no LLM pass over the transcript itself. The only LLM call is the summary (`generate_summary` in `src-tauri/src/llm.rs`): one request per file, prompt authored in English, output language enforced via the summary-language setting, fixed sampling (temperature 0.3, 8192 max tokens — not user-configurable). Podcast episode metadata (feed, title, date) is passed as orientation context. **Without an API key the summary is skipped silently** (`AppConfig::summary_enabled`), so runs work fully offline.

## Podcast Feeds

`src-tauri/src/podcast.rs` parses RSS/Atom via `feed-rs` and only lists entries with an audio enclosure. Episodes are downloaded lazily into a self-deleting temp file inside the Whisper stage — the bounded-pipeline contract (one Whisper + one LLM job in flight) is unaffected.

## Code of Conduct

See [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).
