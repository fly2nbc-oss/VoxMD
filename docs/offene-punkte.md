# Offene Punkte

Stand: 2026-08-03 (nach Umsetzung der empfohlenen Punkte; **#7** bewusst offen).

Diese Liste ist der Rest eines Code- und UX-Reviews. Erledigt in diesem Durchgang:
#1 CSP, #2 Updater-Konvention verworfen (dokumentiert), #3 SECURITY Klartext-Hinweis,
#4 Pipeline-Tests, #5 model_download, #6 Vitest, #9a/#9b stille Degradierungen,
#10a–c Cargo-Profile/Features, #11 LLM-Retry verifiziert (0.41: max 3 Retries),
#12 PLAN.md entfernt, Nebenbefund `react.svg`.

---

## Offen

### #7 — Podcast-Dialog: Episoden auswählen statt alle einreihen

**Datei:** `src/components/PodcastDialog.tsx`

Nach dem Laden des Feeds eine Episodenliste mit Checkboxen zeigen (Default: neueste 10),
erst beim Bestätigen einreihen. Backend liefert die Liste bereits vollständig.

### #8 — Überschreiben-Option für „Skipped (exists)"

Toolbar-Toggle oder Zeilenaktion „Neu erzeugen" mit Rückfrage; Legacy-Dateinamen
aus 1.0.1 mitberücksichtigen.

### #10d / #10e — edition 2024 / crate-type

Bewusst nicht angefasst (geringer Nutzen / Tauri-Build-Risiko).

### Keyring für API-Key

Optional und deutlich größer als die SECURITY.md-Dokumentation — eigenes Vorhaben.
