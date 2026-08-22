import type { MessageKey } from "../i18n";

/**
 * Third-party work VoxMD ships or downloads, with the licence each one is under.
 *
 * Direct dependencies and the models, not the full transitive tree — a list
 * nobody reads credits nobody. Licences were read from the crates in the local
 * registry and from the model cards, not from memory; when a crate offers a
 * choice ("MIT OR Apache-2.0") both are named, because the choice is ours to
 * make and the reader may care which.
 */
export interface Credit {
  name: string;
  license: string;
  url: string;
  /** Only where it is not obvious from the name. */
  note?: MessageKey;
}

export interface CreditGroup {
  /** Section heading. */
  title: MessageKey;
  items: Credit[];
}

export const CREDITS: CreditGroup[] = [
  {
    title: "credits.speech",
    items: [
      {
        name: "whisper.cpp",
        license: "MIT",
        url: "https://github.com/ggerganov/whisper.cpp",
        note: "credits.whisperCpp",
      },
      { name: "whisper-rs", license: "Unlicense", url: "https://github.com/tazz4843/whisper-rs" },
      {
        name: "Whisper",
        license: "Apache-2.0",
        url: "https://huggingface.co/openai/whisper-large-v3",
        note: "credits.whisperModel",
      },
      {
        name: "Symphonia",
        license: "MPL-2.0",
        url: "https://github.com/pdeljanov/Symphonia",
        note: "credits.symphonia",
      },
      { name: "cpal", license: "Apache-2.0", url: "https://github.com/RustAudio/cpal" },
      { name: "Lofty", license: "MIT / Apache-2.0", url: "https://github.com/Serial-ATA/lofty-rs" },
    ],
  },
  {
    title: "credits.speakers",
    items: [
      {
        name: "pyannote segmentation-3.0",
        license: "MIT",
        url: "https://huggingface.co/pyannote/segmentation-3.0",
      },
      {
        name: "WeSpeaker CAM++",
        license: "Apache-2.0",
        url: "https://github.com/wenet-e2e/wespeaker",
        note: "credits.wespeaker",
      },
      { name: "pyannote-rs", license: "MIT", url: "https://github.com/thewh1teagle/pyannote-rs" },
      {
        name: "ONNX Runtime",
        license: "MIT",
        url: "https://github.com/microsoft/onnxruntime",
        note: "credits.onnxruntime",
      },
      { name: "ort", license: "MIT / Apache-2.0", url: "https://github.com/pykeio/ort" },
      { name: "ndarray", license: "MIT / Apache-2.0", url: "https://github.com/rust-ndarray/ndarray" },
    ],
  },
  {
    title: "credits.app",
    items: [
      { name: "Tauri", license: "MIT / Apache-2.0", url: "https://tauri.app" },
      { name: "Tokio", license: "MIT", url: "https://tokio.rs" },
      { name: "reqwest", license: "MIT / Apache-2.0", url: "https://github.com/seanmonstar/reqwest" },
      { name: "async-openai", license: "MIT", url: "https://github.com/64bit/async-openai" },
      { name: "feed-rs", license: "MIT", url: "https://github.com/feed-rs/feed-rs" },
      { name: "Serde", license: "MIT / Apache-2.0", url: "https://serde.rs" },
      { name: "keepawake", license: "MIT", url: "https://github.com/segevfiner/keepawake-rs" },
    ],
  },
  {
    title: "credits.ui",
    items: [
      { name: "React", license: "MIT", url: "https://react.dev" },
      { name: "Lucide", license: "ISC", url: "https://lucide.dev" },
      { name: "Vite", license: "MIT", url: "https://vite.dev" },
      { name: "Vitest", license: "MIT", url: "https://vitest.dev" },
    ],
  },
];

/** Flat list, for tests and for anything that needs the whole set. */
export function allCredits(): Credit[] {
  return CREDITS.flatMap((g) => g.items);
}
