# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

### Fixed

- **The release notes did not mention the `.rpm` at all and called only the Windows build portable**, which made it look as though Linux had no portable download. The AppImage is exactly that; the Downloads table now lists portable builds and installers per platform, states the AppImage's glibc floor, and includes the `.rpm`. The README's Quick Start said `.AppImage` was produced only "when bundled successfully" — it has been in every release since 1.0.3.

## [1.0.6] - 2026-08-03

Dependency maintenance. No intended change in behaviour.

### Changed

- **Every open dependency update is now applied.** Minor and patch updates for cargo, npm and the GitHub Actions came in as-is; the four major bumps needed code changes and are listed below.
- **symphonia 0.5 → 0.6.** The decode path in `audio.rs` was ported to the reworked API: probing returns the format reader directly, audio tracks carry their codec parameters in an `Option<CodecParameters>`, decoders come from `make_audio_decoder`, and end of stream is now an `Ok(None)` packet rather than an unexpected-EOF error. Decoded frames are copied straight into a reused interleaved buffer, which drops the hand-rolled `SampleBuffer` bookkeeping — the streaming, anti-aliased resampling from 1.0.4 is unchanged and still covered by its tests.
- **lofty 0.22 → 0.24.** `Tag::year()` is gone; the year in a Markdown filename now comes from `Tag::date()`, which reads both the recording date and the plain year tag.
- **async-openai 0.24 → 0.41.** The chat-completion types moved under `types::chat`, and every API group is now behind its own feature — only `chat-completion` is enabled, so the other endpoints are no longer compiled in.
- **reqwest 0.12 → 0.13,** required because async-openai 0.41 is built on it. Without this the binary would have linked two HTTP stacks; the summary client also could not have been given its request timeout, since the two versions' `Client` types are unrelated. The `rustls-tls` feature is called `rustls` in 0.13.
- **vite 7 → 8 and @vitejs/plugin-react 4 → 6.** These had to move together: each on its own fails to install against the other's peer range, which is why the individual update PRs could not pass CI. The plugin no longer pulls Babel in, so the frontend dependency tree is smaller.

### Notes

- **TypeScript stays on 5.8.** typescript-eslint pins its TypeScript peer to `>=4.8.4 <6.1.0` on every channel including canary, so TypeScript 7 cannot be installed next to the lint setup. Major TypeScript updates are ignored in `dependabot.yml` until that changes.
- The 1.0.4 entry below never had a tag of its own: it was published as part of `v1.0.5`, so there is no `v1.0.4` release to download. The audio fixes it describes are in every build from 1.0.5 onward.

## [1.0.5] - 2026-08-03

Internal restructuring. No intended change in behaviour, with two small exceptions noted below.

### Changed

- `App.tsx` went from 1557 lines and 28 pieces of state to 510 lines of composition. The UI is now split into components (toolbar, queue table, status bar, settings drawer, podcast dialog, about dialog, error panel, language picker) and hooks (`useTheme`, `useConfigStore`, `useBatchEvents`, `useNativeDrop`), with the pure logic moved to `src/lib/`. The two language settings shared a near-identical block; they now use one component.
- The IPC payload types live in `src/types.ts` alongside the rest of the frontend/backend contract instead of being redeclared.
- Around 30 inline style objects moved into the stylesheet; the handful left are genuinely dynamic (column widths, progress bar width).

### Fixed

- The Whisper model dropdown briefly showed the custom-path field with a preset name in it while the model list was still loading.
- Opening and quickly closing the settings drawer could write state after it had closed.

### Added

- A test asserting that the supported-extension lists in `meta.rs` and the frontend stay identical. They are necessarily duplicated — the backend uses its copy to decide which feed enclosures count as audio — and nothing previously caught them drifting apart.

## [1.0.4] - 2026-08-03

### Fixed

- **Long recordings could exhaust memory.** Audio was fully decoded at its source rate and then resampled into a second complete buffer, so a three-hour 48 kHz episode needed well over 2 GB before transcription even started. Decoding, filtering and resampling now run as a stream and only the 16 kHz result is held. Measured on 20 minutes of 48 kHz stereo, peak memory dropped from roughly 307 MB to 81 MB; for a three-hour episode that is about 2.7 GB down to 0.7 GB.
- **Resampling introduced aliasing, which degraded transcription quality.** Converting to 16 kHz used plain interpolation with no low-pass, so everything above 8 kHz folded back into the speech band — a 12 kHz component landed at 4 kHz, right where speech lives. A 6th-order Butterworth filter now runs before decimation, attenuating that example by more than 30 dB. Audio recorded at 16 kHz is unaffected, and upsampling skips the filter since it cannot alias.
- **Chained streams were decoded incorrectly.** A reset from the demuxer was skipped instead of resetting the decoder, which produces garbage on the chained Ogg files that some feeds serve.
- A per-packet buffer allocation in the decode loop is now reused; a long file decodes into hundreds of thousands of packets.

### Added

- Tests for the audio pipeline, which previously had none: end-to-end decoding of a generated WAV, the anti-aliasing behaviour, resampling in both directions, stereo downmixing, and the error paths.

## [1.0.3] - 2026-08-03

### Security

- **The Windows Vulkan stub loaded `vulkan-1.dll` using the default search order**, which includes the directory of the running executable. Since releases ship a portable `VoxMD.exe` that users typically drop into a folder like Downloads, a `vulkan-1.dll` placed alongside it would have been loaded into the process. It is now resolved from System32 only.
- **Updated `feed-rs` to 2.4, pulling in `quick-xml` 0.41.** The previous version was affected by RUSTSEC-2026-0194 and RUSTSEC-2026-0195 (both rated 7.5): quadratic parsing time on duplicate attribute names, and unbounded allocation for namespace declarations leading to memory exhaustion. VoxMD parses podcast feeds fetched from arbitrary URLs, so this code path handles untrusted input directly.
- Updated `quinn-proto` to 0.11.16 for RUSTSEC-2026-0185 (remote memory exhaustion), and `plist`/`tauri-build` to drop a second affected `quick-xml` copy from the build dependencies. `cargo audit` now reports no vulnerabilities.

### Fixed

- **The Vulkan stub's one-time loader init was unsynchronised.** It used a plain `static int` guard while ggml-vulkan calls in from several threads, so two threads could run the loader concurrently and race on the resolved handles. It now uses `pthread_once` / `InitOnceExecuteOnce`.
- `_GNU_SOURCE` was defined after a libc header in the stub, which made it a no-op.
- **A tag could publish code that was never tested.** The release workflow had no dependency on the lint and test job, so pushing `v*` went straight to building and publishing.
- **The two build jobs raced to create the same GitHub release**, and the checksum files were attached by tag while the bundles were attached by the release the action happened to create — they could land on different releases. The release is now created once as a draft, both platforms upload into it, and it is only published after every platform has produced artifacts.
- **A tag whose number disagreed with the manifests was accepted.** The release now fails immediately unless the tag matches `tauri.conf.json`, `package.json` and `Cargo.toml`.
- The release notes were a fixed template; they are now taken from this changelog.
- The Linux checksum file listed nested build paths while the Windows one listed bare filenames; both now use bare filenames matching the released assets. An empty checksum file is now an error instead of being uploaded.

### Added

- **`rust-toolchain.toml` pins the Rust version** for CI and local checkouts alike. CI floated on `stable` while developers used whatever they had installed, so a lint introduced in 1.97 was invisible on a 1.94 workstation and only appeared as a failed pipeline.
- **ESLint**, wired into the job that was already named "Lint & Test" but only type-checked. Includes the React hooks rules that catch the stale-closure and effect-purity mistakes fixed in 1.0.2.
- **`cargo audit`** as its own CI job.
- **CI now compiles and tests the `gpu-vulkan` feature.** Releases are built with it, but nothing checked that configuration before the slow build jobs, so a break in the Vulkan runtime probe or the loader stub surfaced late.
- **CI asserts that `libvulkan` stays out of `DT_NEEDED`.** The loader stub exists so the app still starts on machines without a Vulkan loader; nothing previously verified that guarantee held.
- `.nvmrc` pins the Node version instead of floating on `lts/*`.

### Changed

- Cargo commands run with `--locked`, so CI cannot silently resolve different dependencies than the committed lockfile.
- All jobs have explicit timeouts. The Windows build compiles whisper.cpp single-threaded, so a hang previously ran until the six-hour default.
- CI declares read-only permissions; only the release jobs that need to write do so.
- Removed the signing secrets from the CI build job. Nothing consumes them — there is no updater plugin configured — so they were exposed on every pull-request build for no benefit.
- Package metadata and the window capability description are in English, matching the rest of the project. The crate description in particular ends up in `.deb` and installer metadata.

### Removed

- The unused Vite/Tauri template favicons in `public/`. The 0.9.8 entry below claims these were deleted, but they were still present and shipped in every build.

## [1.0.2] - 2026-08-03

### Fixed

- **Rows could stay on "Wait" forever**: the queue was seeded *after* the backend had already started reporting, so progress events that arrived in between were discarded — most visibly for episodes skipped as already existing.
- **Unsaved settings were applied and could be persisted by accident**: closing the drawer without saving kept the edits in the live config, so the next run used them and the next toolbar toggle wrote them to disk. In particular "Reset defaults" followed by any toggle silently erased the saved podcast feeds. Closing now discards unsaved edits, and Reset keeps the feed history.
- **Stale settings could overwrite newer ones**: adding a podcast wrote back a config captured before the feed request, discarding anything changed while it ran.
- **A crash in the UI showed a blank window**; there is now an error screen with a reload button.
- **The Cancel indicator flipped back to "Running"** as soon as any further progress arrived, because it was detected by comparing the status text to a literal.
- Previously completed rows were reset to "Wait" when a second batch was started on a subset of the queue.
- The overall tally kept showing a finished batch's numbers after the queue changed.
- An event listener was never removed, and none were removed if the window closed while they were still being registered.
- The file and folder pickers could reject without any feedback, leaving the button apparently dead.
- Starting a batch before the settings finished loading silently used defaults instead of the stored configuration.

### Added

- **Open the result**: finished rows now have buttons to open the generated Markdown file or show it in the file manager. Previously the path was only printed as text.
- **Failed entries are listed** in a dismissible panel instead of scrolling past in the single-line status field.
- **Live transcription percentage** in the Details column, replacing the static "Transcribing…".
- Running state is restored from the backend on startup, so the UI cannot get stuck behind a batch that already finished.

### Changed

- **Dialogs are now real dialogs**: Escape closes them, focus moves into the panel and returns to the trigger afterwards, Tab is trapped inside, and each is labelled for screen readers. Previously Tab walked out into the toolbar underneath, which stayed operable behind the overlay.
- **Keyboard focus is visible**: only text inputs had a focus style, which left the transparent icon buttons with no usable indicator.
- Checkboxes, radio buttons, dropdowns and scrollbars follow the dark theme instead of rendering as light controls on a dark surface.
- The window no longer flashes white on startup in dark mode.
- Status changes are announced to screen readers, and the progress bar exposes its value.
- Status badges are colour-coded by meaning — blue while working, green done, red failed, orange skipped — instead of reusing one colour for both an active and a terminal state.
- Transitions are no longer applied to every element on the page, and `prefers-reduced-motion` is honoured.

## [1.0.1] - 2026-08-03

### Fixed

- **App could wedge until restart**: a panic in the transcription task left the internal processing flag set and emitted no `batch_complete`, so every later Start failed with "Processing is already running." while the UI kept spinning. The flag is now released by a drop guard on every exit path, and exactly one `batch_complete` is always emitted.
- **Podcast episodes could overwrite each other**: episode filenames used only the publication *year*, so a daily show mapped every episode of a year onto one Markdown file and all but the first were silently reported as "skipped (exists)". Names now carry the full `YYYY-MM-DD`. Files written by earlier versions keep their old name and are still recognised, so existing libraries are not re-transcribed.
- **Source audio could be deleted after writing an empty file**: an empty LLM response was treated as a successful empty summary, which with the transcript disabled produced a Markdown file containing nothing but its heading — and then deleted the audio if that option was on. Empty completions are now an error and the source is kept.
- **Truncated audio was transcribed as if complete**: any I/O error during decoding silently ended the decode loop and the partial transcript was written as a success. Only a clean end of stream stops decoding now.
- **Downloads could hang forever**: the Whisper model download had no timeout at all, and the episode download had no read timeout, so a stalled connection blocked the batch with no way out but killing the app. Both now have connect and read timeouts.
- **Incomplete downloads were cached as valid**: a server closing a response early produced a truncated model or episode file that was renamed to its final name and reused on every later run. The transferred size is now verified against `Content-Length`.
- **Cancel had no effect during long operations**: it was only checked between files, so it could not interrupt a running transcription or an in-progress download. Whisper now aborts mid-inference and both download loops stop promptly.
- **Cancel was lost right after Start**: the processing slot is claimed before `start_transcription` returns, so a cancel pressed immediately is no longer discarded.
- **Progress bar never reached 100% when an item failed**: only successful items advanced the counter. Failed items now count as settled.
- **Leftover partial files**: a failed episode download left a `.part` file in the output folder and a failed model download left up to 3 GB of `.tmp` in the cache. Both are cleaned up now.
- Episode and file names derived from feeds or tags are length-capped, stripped of control characters, and no longer collide with Windows device names such as `CON`.

### Added

- Real percentage progress during transcription, replacing the static "Transcribing…" stage.
- Tests for filename derivation from untrusted feed input, including directory-traversal attempts.

### Changed

- The Whisper model download now reports progress once per percent instead of once per chunk, which for a 3 GB model meant hundreds of thousands of UI events.
- Markdown is written on the blocking pool rather than on a runtime worker.
- The locale test no longer mutates process-wide environment variables, which made it order-dependent and prone to failing in CI.

## [1.0.0] - 2026-08-02

### Added

- **Podcast feeds**: toolbar button loads an RSS/Atom feed (`feed-rs`) and queues all episodes with audio enclosures. On Start, audio is downloaded into the chosen output folder (same stem as the Markdown); both files stay there unless the delete-audio toggle is on.
- **Recent podcast feeds** in the podcast dialog (feed URL + output folder, up to 10); click to reuse, trash icon to remove.
- **Queue selection**: checkbox column with select-all and a **Remove** toolbar button. **Start** processes only the selected entries when any are checked (otherwise the full queue).
- **Markdown output toggles** in the toolbar: metadata, summary, and transcript (at least one of summary/transcript required). With the summary disabled, no API key is needed.
- **Appearance**: Light / Dark / System theme in Settings (persisted locally).
- **Transcription language**: Auto-detect or ISO code (same pattern as summary language); default `auto`.
- **Whisper Custom path…**: browse or paste an absolute path to a local `.bin` / `.gguf` model.
- **Drag & drop**: audio files dropped anywhere in the window are added to the queue (same formats as the file picker; unsupported items are ignored with a note).
- New job stage **Download** with percentage progress for podcast episodes.
- Vulkan builds load the Vulkan library at runtime via a stub — missing `libvulkan.so` no longer prevents startup (CPU fallback); `vulkan_status` reports runtime availability.
- `SECURITY.md`, Dependabot config (GitHub Actions, Cargo, npm — weekly, grouped).

### Changed

- **Summary requires an API key**: without one it is skipped silently instead of failing the run (`AppConfig::summary_enabled`); the API fields in Settings are greyed out while the summary is disabled.
- **Delete audio after success** (trash icon in the toolbar): deletes **audio only** after a successful Markdown export (local files and podcast downloads). **Markdown is never deleted.** Default is off (keep audio).
- Theme moved into Settings (**Appearance**); **About** remains a toolbar info icon (separate dialog).
- Settings reorganized into **Summary (LLM)** / **Transcription (Whisper)** / **Appearance** with clearer field explanations.
- Settings **Save** now shows a confirmation state and closes the panel; errors keep it open.
- **Files** and **Podcast** now append to the queue (deduplicated) instead of replacing it.
- Summary prompt rewritten: no speaker labels, quotes cite `[HH:MM:SS]` timestamps, sections are omitted when empty, episode metadata is passed as context, fixed sampling (temperature 0.3).
- Transcript section in the output is the raw Whisper text under `## Transcript` (was the LLM-labelled text under `## Original Transcript`).
- Markdown filenames keep dots in titles (`Path::with_extension` no longer truncates e.g. `Ep. 5 - …`).
- CI: new lint/test gate job (tsc, `cargo fmt`, `clippy -D warnings`, `cargo test`) before the build matrix; concurrency groups cancel superseded runs.

### Removed

- **Speaker detection / labelling** (LLM transcript pass, chunking, format validation, repair pass) — the LLM is now only used for the summary.
- **Transcript chunk size** setting (`transcriptChunkChars`) — obsolete without the transcript LLM pass.
- **Temperature**, **Max tokens**, and **Whisper CPU threads** settings — the summary uses fixed sampling (0.3 / 8192) and Whisper auto-detects threads (cores − 1). Old stored values are dropped on load.
- **Folder** toolbar button and the `collect_audio_in_directory` command (use multi-select in the file picker; `walkdir` dependency dropped).

## [0.9.8] - 2026-05-25

### Fixed

- Config validation: API URL scheme, numeric bounds, local Whisper paths (`.bin`/`.gguf`), non-UTF-8 model path rejection.
- Pipeline: surface source-file deletion failures in job status; reject non-UTF-8 Whisper paths explicitly.
- Frontend: clearer error messages (`toMsg`), guarded numeric settings inputs, warn when model list unavailable.
- Directory scan: canonicalize folder paths before walking.

### Changed

- Remove unused Vite/Tauri template assets in `public/`.

### Removed

- Default `public/` SVG favicons (app icons remain under `src-tauri/icons/`).

## [0.9.7] - 2026-05-24

### Changed

- Repository is **public**; README dynamic release/CI badges work via shields.io.
- Release workflow publishes GitHub releases immediately (`releaseDraft: false`).
- CI: Windows `rust-cache` post-step no longer fails (`cache-targets: false`, `CARGO_TARGET_DIR` set before cache).

### Removed

- **Whisper verbose output** setting.
- `VoxMD.jpg` and `.cursor/plans/` from version control; expanded `.gitignore` for secrets, builds, and local IDE data.

## [0.9.6] - 2026-05-24

### Added

- Settings: **Summary language** — default uses the system locale (`system`); optional fixed ISO 639-1 code (independent of Whisper transcription language).

### Changed

- Windows release: portable executable is published as **`VoxMD.exe`** (no versioned portable filename).

## [0.9.0] - 2026-05-17

### Added

- Tauri v2 desktop shell (Rust backend, React + TypeScript + Vite frontend).
- Local transcription via **whisper.cpp** through `whisper-rs` (no cloud ASR).
- LLM post-processing over an OpenAI-compatible HTTP API: speaker-labelled transcript plus structured summary (`async-openai`).
- Bounded pipeline: concurrent **Whisper** and **LLM** stages with capacity **one** each (`tokio::sync::mpsc` channel buffer = 1).
- Progress events: `job_progress`, `batch_complete`, and `model_download_progress` for Hugging Face GGUF downloads.
- Automatic Whisper GGUF resolution: preset names download into `~/.cache/voxmd/whisper/`; settings dropdown lists presets, sizes, cache markers, and **Clear cache**.
- Supported containers/codecs: MP3, M4A, MP4, WAV, OGG, FLAC, WebM, OPUS (Symphonia decode).
- Persistent configuration via `@tauri-apps/plugin-store` (API URL/key/model, chunk sizes, Whisper options, delete-after-success).
- Optional GPU inference path via Cargo feature **`gpu-vulkan`** (Vulkan toolchain/SDK required at link time).
- GitHub Actions **CI** workflow (`ci.yml`) on `main` / pull requests (Linux + Windows builds).
- GitHub Actions **release** workflow (`tauri-release.yml`) on `v*` tags (Linux + Windows artifacts, per-platform SHA256 checksum files).

### Changed

- English UI copy; unified compact **app bar**; job table columns **File**, **Status**, **Details** (blue badge styling for LLM stage).
- Default configuration aligned with external scripting conventions (Deepseek defaults, `turbo` Whisper preset, delete-after-success enabled).
- README / docs describe CI scope (Linux and Windows builds).

### Fixed

- LLM transcript chunks: split raw text on Whisper timestamp lines when possible; carry trailing labelled lines into the next chunk for speaker consistency; validate `[HH:MM:SS] **Label:**` lines and monotonic timestamps per chunk; automatic single retry with repair prompt before failing the job.
- Packaging hygiene: RGBA icons for Tauri bundle; CI and releases target **Linux and Windows**.
