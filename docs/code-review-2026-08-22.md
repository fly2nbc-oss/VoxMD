# Code-Review der Branch-Änderungen

Stand: 2026-08-22
Verglichen wurde `main...HEAD` (`cursor/cloud-agent-1787269411904-7jzdj`).

Zwei unabhängige Durchgänge wurden zusammengeführt. Alle Punkte sind umgesetzt;
`Fix:` nennt jeweils die Stelle, an der die Korrektur sitzt.

## P1 — Beim Stoppen der Diktierfunktion geht der letzte Text verloren

**Betroffene Stellen:** `src-tauri/src/dictation.rs`, `src/components/DictationView.tsx`

`run_loop` finalisiert den Restpuffer nur, wenn `STOP` nicht gesetzt ist. Der
normale Stop setzt dieses Flag jedoch immer, der Block war damit toter Code.
Zusätzlich meldete das Abbruch-Callback von `transcribe_buffer` ebenfalls `STOP`,
sodass Whisper auch bei erreichtem Block sofort zurückgekehrt wäre. Anschließend
löschte die Oberfläche beim Ereignis `stopped` den angezeigten Partial-Text.
Dadurch konnten bis zu 25 Sekunden Diktat verschwinden.

**Fix:** Die `!STOP`-Bedingung entfällt, und `transcribe_buffer` bekommt das
Abbruchprädikat als Parameter — die Finalisierung übergibt `|| false`.

## P1 — Die Zeitachse der Speaker-Diarization driftet bei längeren Aufnahmen

**Betroffene Stelle:** `src-tauri/src/diarize.rs`

`offset` wurde einmal initialisiert und anschließend über alle 10-Sekunden-Fenster
weitergezählt, ohne den Fensteranfang `start` einzubeziehen. Gegen das Modell
gemessen liefert `segmentation-3.0` für ein Fenster von 160 000 Samples die Form
`[1, 589, 7]`, also 589 Frames à 270 Samples = 159 030 Samples. Der Zähler bleibt
damit pro Fenster 970 Samples (≈ 61 ms) zurück:

| Audiolänge | Fenster | Versatz |
|---|---|---|
| 10 min | 60 | 1,0 s |
| 30 min | 180 | 10,9 s |
| 1 h | 360 | 21,8 s |
| 3 h | 1080 | 65 s |

Das wirkt doppelt: `speakers_for_lines` ordnet korrekte Whisper-Zeitstempel
gestauchten Turn-Zeiten zu, und `make_seg` schneidet die Embedding-Samples mit
denselben verschobenen Indizes aus. Beides verschlechtert die Sprecherzuordnung
im hinteren Teil langer Aufnahmen erheblich und ist eine plausible Mitursache des
im CHANGELOG genannten Problems mit Interviews.

**Fix:** `frame_offset(window_start, frame)` verankert das Raster an jedem
Fenster; der Zusammenhang ist als Regressionstest festgehalten und als Punkt 5
in der Liste der Upstream-Fehler von `speech_segments` dokumentiert.

## P2 — Ein erfolgreich bestätigtes Append kann am Batch-Ende verloren gehen

**Betroffene Stelle:** `src-tauri/src/pipeline.rs`

Der Worker prüfte die leere Queue und beendete sich danach, ohne atomar zu
verhindern, dass `append_to_batch` noch einen Eintrag annimmt. Erfolgte das
Append zwischen Leerprüfung und Beenden, lieferte der Befehl Erfolg, der
`ProcessingGuard` verwarf den Eintrag anschließend jedoch unverarbeitet.

**Fix:** `PENDING` hält jetzt `Pending { queue, closed }` unter einem Lock.
`try_finish_pending` schließt die Queue nur, wenn sie leer und nichts in Arbeit
ist; `append_to_batch` prüft `closed` unter demselben Lock und meldet sonst
„The batch is already finishing." statt still zu verwerfen.

## P2 — Live hinzugefügte Dateien können doppelt verarbeitet werden

**Betroffene Stelle:** `src/App.tsx`

`setJobs` und `invoke("append_to_batch")` liefen innerhalb eines
React-State-Updaters. Solche Updater müssen frei von Nebenwirkungen sein und
werden durch das aktive `React.StrictMode` in Entwicklungs-Builds mehrfach
ausgeführt.

**Fix:** Ein `itemsRef` spiegelt die Queue, Deduplizierung und IPC laufen vor den
`set*`-Aufrufen.

## P2 — Der Diktat-Zustand kann dauerhaft hängenbleiben

**Betroffene Stellen:** `src/App.tsx`, `src/components/DictationView.tsx`

Die `dictation_status`-Listener lebten nur in `DictationView`. Ctrl+1 ruft
`switchMode("queue")`, das `stopDictation()` aufruft und die Komponente im selben
Commit unmountet — das `stopped`-Ereignis trifft erst Sekunden später ein und
findet keinen Listener mehr. `dictating` blieb `true`, der Batch-Start war mit
„Stop dictation before starting a batch" blockiert, und ein erneutes Stop war
wirkungslos, weil der Backend-Thread bereits beendet war.

**Fix:** Neuer Hook `useDictationEvents` auf App-Ebene; `DictationView` liest den
Zustand als Prop.

## P2 — Cancel während Download oder vor Whisper lässt die Zeile hängen

**Betroffene Stelle:** `src-tauri/src/pipeline.rs`

Zwei der vier Abbruchpfade riefen nur `emit_skipped_remaining` und brachen ab,
ohne für das aktuelle Element ein Terminal-Ereignis zu senden. Die Zeile blieb
dauerhaft auf „Download" beziehungsweise „Whisper 0 %" stehen.

**Fix:** Beide Stellen nutzen `emit_cancelled`, wie die übrigen Pfade.

## P2 — Bereits vorhandene Exporte werden dauerhaft wiederhergestellt

**Betroffene Stellen:** `src/lib/queuePersist.ts`, `src-tauri/src/pipeline.rs`

Die Queue-Persistenz entfernte ausschließlich Jobs mit Stage `done`. Ein Job mit
`Skipped (exists)` ist ebenfalls abgeschlossen, wurde aber gespeichert und bei
jedem Programmstart erneut als wartend angezeigt. Abgebrochene Jobs müssen
weiterhin gespeichert werden, die beiden `skipped`-Ursachen waren jedoch nicht
unterscheidbar.

**Fix:** `JobProgressPayload` trägt jetzt ein Feld `outputPath`, gesetzt bei
`done` und bei `Skipped (exists)`. Das ersetzt zugleich das Parsen des
Statustextes in `outputPathOf` (die alte Zerlegung bleibt als Fallback für
gespeicherte Zeilen älterer Versionen).

## P2 — Die angekündigte AIFF-Unterstützung schließt `.aif`-Dateien aus

**Betroffene Stellen:** `src/lib/queue.ts`, `src-tauri/src/meta.rs`

Frontend und Backend akzeptierten nur die Erweiterung `.aiff`.

**Fix:** `.aif` ergänzt; der vorhandene Paritätstest deckt beide Listen ab.

## P2 — Map-Reduce der Zusammenfassung hat keinen Overflow-Fallback

**Betroffene Stelle:** `src-tauri/src/llm.rs`

`looks_like_context_overflow` wurde nur im Einzelaufruf ausgewertet. Bei einem
sehr langen Transkript ergibt `total/16` Teile von deutlich über 120 000 Zeichen,
die ein Modell mit kleinem Kontext ebenfalls ablehnt — die Datei schlug komplett
fehl.

**Fix:** `summarize_chunked` halbiert das Teilbudget und wiederholt, bis
`SUMMARY_MIN_PART_CHARS` erreicht ist. `SUMMARY_MAX_PARTS` ist dabei ein weiches
Limit.

## P2 — `max_tokens` bricht bei Reasoning-Modellen

**Betroffene Stelle:** `src-tauri/src/llm.rs`

Mit OpenRouter als Provider ist ein Modell wie `openai/o3` zwei Klicks entfernt.
Diese Modelle lehnen `max_tokens` und ein abweichendes `temperature` ab.

**Fix:** `call_llm` erkennt solche Ablehnungen und wiederholt den Aufruf genau
einmal ohne beide Felder.

## P3 — Speicherspitze der Diarisierung

**Betroffene Stelle:** `src-tauri/src/diarize.rs`

Für eine Zwei-Stunden-Episode kamen zusätzlich zum Whisper-Modell rund 1,15 GB
zusammen: `samples_f32` (461 MB), `to_i16` (230 MB), der `padded`-Klon (230 MB)
und die Sample-Kopien in `SpeechSeg` (bis 230 MB).

**Fix:** `SpeechSeg` hält einen Indexbereich statt einer Kopie, und nur das
letzte Teilfenster wird in einen kleinen Scratch-Puffer gepolstert. Das spart
rund 460 MB.

## P3 — Resampler-Zustand wird bei jedem Diktat-Chunk verworfen

**Betroffene Stellen:** `src-tauri/src/audio.rs`, `src-tauri/src/dictation.rs`

`resample_mono_to_16k` legte pro Aufruf einen frischen `Resampler` an, obwohl der
Typ Zustand hält (drei Biquad-Sektionen, `pending`, gebrochene Leseposition).
Im Diktat-Loop geschah das alle 1,2 s und erzeugte an jeder Chunk-Grenze einen
Einschwingvorgang plus Phasensprung.

**Fix:** `resampler_to_16k` liefert eine Instanz für die ganze Sitzung,
`Resampler::take` gibt das bisher Resampelte heraus, ohne den Zustand zu
verlieren.

## P3 — Der Audio-Callback macht IPC-Arbeit

**Betroffene Stelle:** `src-tauri/src/dictation.rs`

`maybe_emit_level` nahm ein Mutex und rief alle 50 ms `app.emit` samt
JSON-Serialisierung direkt im cpal-Callback auf.

**Fix:** Der Callback schreibt den RMS-Wert nur noch in ein `AtomicU32`; die
ohnehin vorhandene Polling-Schleife sendet ihn.

## P3 — Blockierende Arbeit in synchronen Tauri-Commands

**Betroffene Stelle:** `src-tauri/src/lib.rs`

`list_microphones`, `start_mic_monitor`, `stop_mic_monitor`,
`list_whisper_models`, `whisper_cache_dir`, `clear_whisper_cache` und
`vulkan_status` liefen ohne `async` auf dem Main-Thread. `stop_mic_monitor`
joint dabei einen Thread.

**Fix:** Alle sieben sind jetzt `#[tauri::command(async)]`.

## P3 — Wettlauf zwischen Batch-Start und Diktat

**Betroffene Stellen:** `src-tauri/src/lib.rs`, `src-tauri/src/dictation.rs`

Prüfung und Reservierung des jeweils anderen Flags waren nicht atomar; beide
hätten gleichzeitig einen `WhisperContext` belegen können.

**Fix:** Beide Seiten reservieren zuerst ihr eigenes Flag und prüfen dann das
andere; der Verlierer gibt via `pipeline::release_batch` beziehungsweise
`DICTATING.store(false)` zurück.

## P3 — Abbruch im LLM-Schritt zählte nicht als erledigt

**Betroffene Stelle:** `src-tauri/src/pipeline.rs`

Die drei Cancel-Pfade in `llm_stage` sendeten `skipped`, erhöhten den
`done_counter` aber nicht, sodass der Gesamtfortschritt stehenblieb.

**Fix:** Gemeinsames `emit_cancel`, das wie `emit_error` mitzählt.

## P3 — Diarisierungsmodelle luden ohne Fortschrittsanzeige

**Betroffene Stelle:** `src-tauri/src/pipeline.rs`

`ensure_models(|_, _| {})` verwarf den Fortschritt für rund 32 MB Download.

**Fix:** Der Callback speist `model_download_progress`, auf ganze Prozent
gedrosselt wie beim Whisper-Modell.

## P3 — `MAX_SPEAKERS_CAP` war dreifach dupliziert

**Betroffene Stellen:** `src-tauri/src/config.rs`, `src/lib/configStore.ts`,
`src/components/SettingsDrawer.tsx`

Für `AUDIO_EXTENSIONS` existiert ein Paritätstest über die Sprachgrenze, für den
Sprecher-Cap nicht.

**Fix:** `MAX_SPEAKERS` in `configStore.ts` als einzige Frontend-Quelle, von
`SettingsDrawer` und `asMaxSpeakers` genutzt, und ein analoger Rust-Test
`speaker_cap_matches_the_frontend`.

## P3 — Paketmetadaten und Feature-Deklarationen

**Betroffene Stellen:** `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`

`cpal` linkt `libasound.so.2`, `keepawake` linkt `libdbus-1.so.3`; das .deb
deklarierte beides nicht. Und `ort` war als `default-features = false,
features = ["ndarray"]` deklariert — `download-binaries` und `copy-dylibs`, die
onnxruntime überhaupt erst beisteuern, kamen laut `cargo tree -e features`
ausschließlich über die Defaults von `pyannote-rs`.

**Fix:** `bundle.linux.deb.depends` ergänzt, ort-Features explizit benannt.

## P3 — Kleinere Aufräumarbeiten

- `agglomerative_cluster` aktualisiert Clusterabstände jetzt über die
  Lance-Williams-Rekurrenz (exakt für Average Linkage) statt sie aus allen
  Rohpaaren neu zu summieren; die zweite n×n-Matrix entfällt.
- `regex_skip_model` enthielt keine Regex und heißt jetzt `is_non_chat_model`.
- `improve_text` / `translate_text` verwendeten das feste Zusammenfassungsbudget
  von 8192 Tokens und schnitten lange Diktate ab; `rewrite_max_tokens` skaliert
  mit der Eingabe.
- Doppeltes `sleep` im Leerlaufzweig der Whisper-Schleife entfernt.
- Der Provider-Wechsel behielt den API-Key der vorherigen Gegenstelle, was
  „Verify" mit einem undurchsichtigen 401 scheitern ließ; die Einstellungen
  weisen jetzt darauf hin.
- Der Translate-Button hatte im Ruhezustand kein Icon.
- Der Einstellungshinweis erwähnt jetzt, dass Diarisierung die Verarbeitungszeit
  spürbar erhöht.

## Nachtrag — Binärgröße

Gemessen per Linker-Map, nicht geschätzt. Die dominierende Einzelposition war
statisch gelinktes onnxruntime mit 18,7 MB, ausschließlich für die
standardmäßig abgeschaltete Sprecher-Erkennung.

| Anteil | vorher (CPU) | vorher (Vulkan) |
|---|---:|---:|
| ggml-vulkan (SPIR-V für alle Quantisierungen) | — | 36,5 MB |
| onnxruntime (statisch) | 18,7 MB | 18,7 MB |
| voxmd + alle Rust-Crates (eine LTO-Einheit) | 14,4 MB | 14,4 MB |
| aws-lc-rs (rustls-Krypto) | 2,4 MB | 2,4 MB |
| whisper.cpp / ggml-cpu | 1,7 MB | 1,7 MB |
| **gesamt** | **39,4 MB** | **75,9 MB** |

`lto = true`, `codegen-units = 1` und `strip = true` waren bereits gesetzt.

**Umgesetzt:** `ort` läuft mit `load-dynamic`; `onnx_runtime.rs` lädt die
passende Microsoft-Release beim ersten Diarisierungslauf nach
`~/.cache/voxmd/diarize/` — also in dasselbe Verzeichnis wie die beiden
Modelle — und prüft den veröffentlichten SHA-256.

| | vorher | nachher |
|---|---:|---:|
| CPU-Build | 39,4 MB | **19,1 MB** (−52 %) |
| Release-Build (Vulkan) | 75,9 MB | **55,6 MB** (−27 %) |

Zusätzlich entfällt der 94-MB-Download des statischen Archivs bei jedem sauberen
Build (`alternative-backend` → `ort-sys/disable-linking`), weil `download-binaries`
über die `ort`-Defaults von `pyannote-rs` hereinkommt und von außen nicht
abschaltbar ist.

Preis: Wer Sprecherlabels einschaltet, lädt einmalig zusätzlich rund 7 MB
(Linux) beziehungsweise 69 MB (Windows-Release-ZIP) und 52 MB (macOS). Die
Windows-Asymmetrie liegt daran, dass Microsofts Release-ZIP neben der 11,8-MB-DLL
Header und Importbibliotheken mitliefert; das PyPI-Wheel enthält dieselbe DLL bei
12,1 MB Download, wurde aber nicht übernommen, weil es eine zweite Bezugsquelle
wäre und hier nicht unter Windows verifizierbar war.

**Nicht umgesetzt, mit Begründung:**

- *ggml-vulkan trimmen (−36,5 MB):* Der Backend backt SPIR-V für alle
  ggml-Quantisierungstypen ein. Ein Subset hieße whisper-rs-sys' CMake patchen,
  und Nutzer mit anderen Quantisierungen verlören GPU-Unterstützung.
  Verlustfreie Alternative wäre, CPU- und GPU-Artefakt getrennt zu
  veröffentlichen.
- *`panic = "abort"` (−~3,7 MB):* `pipeline.rs` verlässt sich darauf, dass ein
  Panic aus `run_batch` entrollt — `ProcessingGuard` gibt den Slot frei,
  `spawn_blocking` fängt ihn ab. Mit `abort` stürbe die App.
- *aws-lc-rs → ring (−2,4 MB):* `rustls` wird von `reqwest`, `async-openai` und
  `rustls-platform-verifier` aktiviert; Feature-Unification macht den Tausch
  fummelig, für 3 % Ersparnis am TLS-Stack.

## Durchgeführte Prüfungen

- Rust-Formatierung (`cargo fmt --check`): erfolgreich
- Rust-Clippy mit `-D warnings`: erfolgreich
- Rust-Tests: 80 bestanden, 0 fehlgeschlagen (74 vorher, 6 neu)
- TypeScript (`tsc --noEmit`): erfolgreich
- ESLint: erfolgreich
- Frontend-Tests: 21 bestanden, 0 fehlgeschlagen (18 vorher, 3 neu)
- Frontend-Produktions-Build: erfolgreich
- `npx tauri build`: Binary und .deb erfolgreich

Der AppImage-Schritt schlägt auf dieser Arch-/Manjaro-Maschine fehl, weil das in
`linuxdeploy` (Build von 2024-07) mitgelieferte `strip` die Sektion `.relr.dyn`
der aktuellen Toolchain nicht kennt. Betroffen sind Systembibliotheken wie
`libXau` und `libcairo`, nicht das Projekt; die CI baut auf `ubuntu-24.04`.

Neu hinzugekommene Tests decken die Frame-Rasterung der Diarisierung, den
Append-Handshake, das Teilbudget der Zusammenfassung, das Rewrite-Token-Budget,
die Erkennung abgelehnter Sampling-Parameter, den Sprecher-Cap-Abgleich, die
`skipped`-Unterscheidung in der Queue-Persistenz und `outputPath` ab.
