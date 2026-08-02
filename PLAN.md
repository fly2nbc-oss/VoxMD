# VoxMD Umbau-Plan (v0.10.0) — historical

> **Historical only.** This plan targeted a 0.10.0 cut from v0.9.8 (2026-06-11).
> The work shipped as **1.0.0** with important divergences: podcast audio is
> persisted in the output folder (not a temp file), Markdown toggles live in the
> toolbar, Appearance/theme is in Settings, About is a toolbar icon, and
> `delete_source_after_success` deletes audio only (default off). Prefer
> `README.md`, `CHANGELOG.md`, and `CLAUDE.md` for current behaviour.

Stand: 2026-06-11 · Basis: v0.9.8 (main)

Neun Änderungen, gruppiert in vier Arbeitspakete. Empfohlene Reihenfolge: **A → B → C → D**, weil Paket A (Speaker-Entfernung) den Code stark vereinfacht und alles Weitere darauf aufbaut.

---

## Paket A — Backend-Vereinfachung

### Punkt 7: Sprechererkennung entfernen

Die LLM-Transkript-Stufe existiert ausschließlich für das Speaker-Labeling. Ohne Speaker wird das Transkript im Output zur **rohen Whisper-Ausgabe** (`[HH:MM:SS] Text`-Zeilen) — die LLM-Stufe macht nur noch die Summary.

**`src-tauri/src/llm.rs`** (≈300 Zeilen entfallen):
- Löschen: `SYSTEM_PROMPT`, `SYSTEM_PROMPT_CONT`, `SYSTEM_PROMPT_REPAIR`, `transcript_with_speakers_with_progress`, `transcribe_one_chunk`, `validate_labeled_chunk`, `tail_labeled_context`, `split_chunks`, `split_long_line`, `byte_boundary_before`, `parse_ts_seconds` + zugehörige Tests.
- Bleibt: `make_client`, `call_llm`, `generate_summary`, `fmt_ts`, `segments_to_raw_text`.

**`src-tauri/src/pipeline.rs`**:
- `llm_stage`: Aufruf von `transcript_with_speakers_with_progress` entfernen; `transcript = job.raw_text`. Statusmeldung „Speakers & summary…" → „Summary…".
- `JobProgressPayload.llm_chunk` entfernen (Chunk-Fortschritt entfällt).

**`src-tauri/src/config.rs` + `src/types.ts` + `src/defaults.ts` + Settings-UI**:
- `transcript_chunk_chars` / `transcriptChunkChars` komplett entfernen (Chunking entfällt; Summary truncated ohnehin bei 50k Zeichen). Validierung in `validate_for_run()` anpassen.

**`src/App.tsx`**: `llmChunk` aus `JobProgressPayload`/`detailsForRow` entfernen („Speakers · chunk x/y" entfällt).

### Punkt 8: LLM-Prompts optimieren

Nur noch ein Prompt: `summary_system_prompt` in `llm.rs`.
- Speaker-Bezüge entfernen (Zitat-Format `**Label:** "Zitat"` → `> "Zitat" [HH:MM:SS]` mit Timestamp-Referenz).
- Kontext mitgeben: Titel (+ Episoden-Metadaten aus Punkt 4/6) im User-Prompt, damit `## Metadata` nicht raten muss.
- Straffen: klare Abschnittsreihenfolge, explizit „kein Preamble, direkt mit `##` beginnen", Mengenangaben präzisieren (Bullets ≤ 10, Quotes 3–8, nur wenn ergiebig).
- Temperatur für Summary fest niedriger ansetzen (z. B. `min(cfg.temperature, 0.4)`) für faktentreue Extraktion.

### Punkt 6: Output-Checkboxen — Meta / Summary / Transcript

Drei neue Config-Felder (Default `true`), in **allen vier Stellen** ergänzen (`config.rs`, `types.ts`, `defaults.ts`, Settings-UI):
- `include_meta` / `includeMeta` — Metadaten-Block am Anfang der MD-Datei (bei Podcast-Episoden: Feed-Titel, Episodentitel, Datum, Link; bei lokalen Dateien: Tags Titel/Jahr).
- `include_summary` / `includeSummary` — bei `false` entfällt der Summary-LLM-Call komplett (kein API-Key-Zwang mehr nötig? → Nein: `validate_for_run()` verlangt API-Daten nur noch, wenn `include_summary == true`).
- `include_transcript` / `includeTranscript` — Transkript-Sektion ein/aus.

Validierung: `include_summary || include_transcript` muss `true` sein (sonst leere Datei).

MD-Assembly in `pipeline.rs`:
```
# {title}
{meta-Block, wenn includeMeta}
{summary, wenn includeSummary}
## Transcript            ← wenn includeTranscript
{raw whisper transcript}
```

---

## Paket B — UI-Umbau (App.tsx)

### Punkt 3: Folder-Button entfernen
- `pickFolder` + Button + `FolderOpen`-Import aus `App.tsx` entfernen.
- Backend aufräumen: `collect_audio_in_directory` aus `lib.rs` (Command + Registrierung) entfernen; `walkdir`-Dependency aus `Cargo.toml` streichen, falls sonst ungenutzt.

### Punkt 1: „Delete source" in die Funktionsleiste
- Checkbox aus dem Settings-Drawer entfernen (`App.tsx:715–722`).
- Stattdessen Toggle-Button in der `app-bar` (Lucide `Trash2`, aktiv/inaktiv-Zustand visuell unterscheidbar, Tooltip „Delete source audio after successful export").
- Klick schreibt sofort via `saveConfig({...config, deleteSourceAfterSuccess: !...})` — persistiert ohne Settings-Dialog.
- `AppConfig`-Feld bleibt unverändert (Backend braucht es weiter).

### Punkt 2: Settings-Save mit Bestätigung + Schließen
- `Save`-Handler: `await saveConfig(config)` → kurzes visuelles Feedback (Button zeigt ✓ „Saved" für ~600 ms) → `setSettingsOpen(false)`.
- Fehlerfall: Drawer bleibt offen, Fehlermeldung im Drawer anzeigen.

### Punkt 5: Auswahl + Löschen von Listeneinträgen
- Neuer State `selected: Set<string>` (Key = `path`/URL).
- Tabelle: neue erste Spalte mit Checkbox pro Zeile; Header-Checkbox = Select all / Deselect all (indeterminate bei Teilauswahl).
- Button „Remove selected" (Lucide `ListX`/`Trash`) oberhalb der Tabelle oder in der Funktionsleiste; entfernt markierte Einträge **nur aus der Queue** (keine Dateien auf Platte).
- Während `processing` deaktiviert; Auswahl wird bei neuem Datei-Import zurückgesetzt.

---

## Paket C — Podcast-Feature (Punkt 4)

Größter Brocken; ändert den Queue-Datentyp.

### Datenmodell
`paths: string[]` wird zu `QueueItem[]`:

```ts
interface QueueItem {
  id: string;                 // path bzw. enclosure-URL
  kind: "local" | "podcast";
  source: string;             // Dateipfad oder Audio-URL
  displayName: string;
  episode?: {                 // nur kind === "podcast"
    feedTitle: string;
    title: string;
    date?: string;            // ISO
    link?: string;
    outputDir: string;        // Zielordner für die .md
  };
}
```

Rust-Gegenstück (serde camelCase) in neuem Modul `src-tauri/src/podcast.rs` bzw. `pipeline.rs`.

### Backend (`src-tauri/src/podcast.rs`, neu)
- Neues Tauri-Command `fetch_podcast_feed(url) -> Vec<EpisodeInfo>`: Feed via `reqwest` laden, mit **`feed-rs`** parsen (RSS + Atom), pro Episode `title`, `pubDate`, `enclosure.url`, `link` extrahieren. Episoden ohne Audio-Enclosure überspringen.
- **Lazy Download**: Episoden werden *nicht* beim Hinzufügen geladen, sondern erst in der Whisper-Stufe — Download in Temp-Datei (`tempfile`-Crate), nach Verarbeitung immer löschen (unabhängig von `deleteSourceAfterSuccess`; der Toggle betrifft nur lokale Dateien).
- `start_transcription` nimmt `Vec<QueueItem>` statt `Vec<String>`; Whisper-Task lädt bei `kind == podcast` zuerst herunter (mit Progress-Event, Stage `"download"`), dann wie gehabt dekodieren.
- MD-Pfad: bei Podcast-Episoden `episode.outputDir / sanitize("{jahr} - {titel}").md` (Logik in `meta.rs` erweitern); Skip-wenn-existiert greift unverändert → idempotent.

### Frontend
- Neuer Button „Podcast" (Lucide `Rss`/`Podcast`) in der Funktionsleiste → kleiner Dialog: Feed-URL-Eingabe + Zielordner-Auswahl (Tauri-Dialog `open({directory:true})`, letzter Ordner wird in der Config gemerkt: neues Feld `podcastOutputDir`).
- Nach Fetch: Episoden als `QueueItem`s in die Liste (neueste zuerst); zusammen mit Punkt 5 kann der Nutzer dann unerwünschte Folgen abwählen/entfernen.
- Status-Badge erweitern: Stage `"download"` → Badge „Download".

### Neue Dependencies (`src-tauri/Cargo.toml`)
`feed-rs`, `tempfile` (reqwest ist über `model_download` vermutlich schon da — prüfen, sonst ergänzen mit `rustls`-Features).

---

## Paket D — Doku & GitHub (Punkt 9)

### Dokumentation aktualisieren (Speaker raus, Podcast rein)
- **README.md**: Zeile 10 (Speaker-Erwähnung), Output-Beispiel Z. 85/88 auf neues Format; Features-Liste um Podcast-RSS, Output-Toggles, Auswahl-UI ergänzen; Settings-Tabelle aktualisieren (transcriptChunkChars raus, neue Checkboxen rein); Troubleshooting-Abschnitt ergänzen.
- **CONTRIBUTING.md**: Abschnitt „LLM Transcript Stage" (Speaker-Contract, Z. 68) ersetzen durch Beschreibung Summary-only + Podcast-Modul.
- **CLAUDE.md**: Architektur-Beschreibung anpassen (LLM-Stufe, `QueueItem`, podcast.rs, entfernte Settings). CHANGELOG-Einträge zu 0.9.0 bleiben unverändert (historisch).
- **CHANGELOG.md**: neuer Eintrag `0.10.0` (Added: Podcast-RSS, Output-Toggles, Listen-Auswahl; Changed: Delete-Source-Toggle in Toolbar, Save-UX; Removed: Speaker-Labeling, Folder-Button, transcriptChunkChars).
- **SECURITY.md** neu anlegen (Supported versions, Meldeweg).

### CI/Build-Best-Practices (`.github/workflows/`)
- **ci.yml**: vorgelagerter schneller `check`-Job (Linux): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run build` (tsc); Build-Matrix `needs: check`. `concurrency`-Gruppe mit `cancel-in-progress: true` für PR-Pushes.
- **tauri-release.yml**: `concurrency`-Gruppe; optional `releaseDraft: true` (manuelles Publish als Qualitäts-Gate) — Entscheidung beim Umsetzen.
- **dependabot.yml** neu: wöchentliche Updates für `github-actions`, `cargo`, `npm`.
- Versionsbump auf **0.10.0** in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` (aktuell konsistent 0.9.8).

---

## Querschnitt & Risiken

| Thema | Entscheidung im Plan |
|---|---|
| Alte gespeicherte Settings | `mergeConfig` + serde-`#[serde(default)]` fangen fehlende neue Felder ab; entferntes `transcriptChunkChars` wird beim nächsten Save einfach nicht mehr geschrieben. |
| Kanal-Invariante (Kapazität 1) | Bleibt unangetastet; Podcast-Download läuft innerhalb der Whisper-Stufe (sequenziell), verletzt den Vertrag nicht. |
| `includeSummary == false` | Kein LLM-Call, kein API-Key nötig → `validate_for_run()` bedingt machen. |
| Podcast + `deleteSourceAfterSuccess` | Temp-Downloads werden immer gelöscht; Toggle wirkt nur auf lokale Dateien. |
| Offene WIP-Änderungen | Working Tree hat unkommittete Änderungen (`audio.rs`, `config.rs`, `llm.rs`, …) — vor Start committen oder verwerfen, damit die Pakete saubere Commits ergeben. |

## Test- & Abnahme-Checkliste

1. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` (in `src-tauri/`)
2. `npm run build` (tsc + Vite)
3. Manuell: lokale Datei → MD mit/ohne Summary/Transcript/Meta-Kombinationen; Podcast-Feed laden, Episoden abwählen, eine Episode verarbeiten; Delete-Source-Toggle in Toolbar; Settings-Save (Bestätigung + Schließen); Cancel während Download/Whisper/LLM.
4. CI grün auf Linux + Windows (inkl. neuem Check-Job).

**Vorgeschlagene Commits:** ein Commit pro Paket (A–D), Release-Tag `v0.10.0` nach Abnahme.
