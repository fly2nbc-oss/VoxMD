# Code-Review der Branch-Änderungen

Stand: 2026-08-22  
Verglichen wurde `main...HEAD` (`cursor/cloud-agent-1787269411904-7jzdj`).

## Findings

### P1 — Beim Stoppen der Diktierfunktion geht der letzte Text verloren

**Betroffene Stellen:** `src-tauri/src/dictation.rs:326`,
`src/components/DictationView.tsx:103`

`run_loop` finalisiert den Restpuffer nur, wenn `STOP` nicht gesetzt ist. Der
normale Stop setzt dieses Flag jedoch immer. Anschließend löscht die Oberfläche
beim Ereignis `stopped` auch den angezeigten Partial-Text. Dadurch können bis zu
25 Sekunden Diktat verschwinden.

### P2 — Die Zeitachse der Speaker-Diarization driftet bei längeren Aufnahmen

**Betroffene Stelle:** `src-tauri/src/diarize.rs:663`

`offset` wird nur einmal initialisiert und anschließend anhand der ausgegebenen
Modellframes über alle 10-Sekunden-Fenster weitergezählt. Der jeweilige
Fensteranfang `start` wird nicht einbezogen. Da die Modellframes ein Fenster
nicht exakt in 270-Sample-Schritten abdecken, summiert sich ein Zeitversatz.
Spätere Whisper-Zeilen können dadurch falschen Sprechern zugeordnet werden.

### P2 — Live hinzugefügte Dateien können doppelt verarbeitet werden

**Betroffene Stelle:** `src/App.tsx:193`

`setJobs` und `invoke("append_to_batch")` werden innerhalb eines React-State-
Updaters ausgeführt. Solche Updater müssen frei von Nebenwirkungen sein und
werden durch das im Projekt aktive `React.StrictMode` in Entwicklungs-Builds
mehrfach ausgeführt. Damit kann derselbe Eintrag mehrfach in der Backend-Queue
landen und parallel zur ersten Zusammenfassung erneut transkribiert werden.

### P2 — Ein erfolgreich bestätigtes Append kann am Batch-Ende verloren gehen

**Betroffene Stellen:** `src-tauri/src/pipeline.rs:101`,
`src-tauri/src/pipeline.rs:666`

Der Worker prüft die leere Queue und beendet sich danach, ohne atomar zu
verhindern, dass `append_to_batch` noch einen Eintrag annimmt. Erfolgt das Append
zwischen der Leerprüfung und dem Beenden, liefert der Befehl Erfolg. Der
`ProcessingGuard` löscht den neuen Eintrag anschließend jedoch, ohne ihn zu
verarbeiten.

### P2 — Bereits vorhandene Exporte werden dauerhaft wiederhergestellt

**Betroffene Stelle:** `src/lib/queuePersist.ts:19`

Die Queue-Persistenz entfernt ausschließlich Jobs mit Stage `done`. Ein Job mit
`Skipped (exists)` ist ebenfalls abgeschlossen, wird aber gespeichert und bei
jedem Programmstart erneut als wartend angezeigt. Abgebrochene Jobs müssen
weiterhin gespeichert werden; die verschiedenen Ursachen für `skipped` sollten
daher unterscheidbar sein.

### P2 — Die angekündigte AIFF-Unterstützung schließt `.aif`-Dateien aus

**Betroffene Stellen:** `src/lib/queue.ts:10`, `src-tauri/src/meta.rs:4`

Frontend und Backend akzeptieren nur die Erweiterung `.aiff`. Dateien mit der
ebenfalls verbreiteten Endung `.aif` werden vom Dateidialog, von Drag-and-drop
und von der Podcast-Erkennung abgewiesen, obwohl der Decoder AIFF unterstützt.

## Durchgeführte Prüfungen

- Frontend-Produktions-Build: erfolgreich
- ESLint: erfolgreich
- Frontend-Tests: 18 bestanden, 0 fehlgeschlagen
- Rust-Formatierung (`cargo fmt --check`): erfolgreich
- Rust-Clippy mit `-D warnings`: erfolgreich
- Rust-Tests: 74 bestanden, 0 fehlgeschlagen
- `git diff --check`: erfolgreich

Die grünen automatischen Prüfungen decken die oben beschriebenen Laufzeit-,
Nebenwirkungs- und Race-Condition-Fälle derzeit nicht ab.
