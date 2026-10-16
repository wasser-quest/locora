# Offen

Alles, was noch gebaut oder entschieden werden muss. Wird bei jedem Schritt aktualisiert. Erledigtes wird hier gestrichen und in `PLAN.md` (Abschnitt 13) bzw. den Commits festgehalten. Reihenfolge innerhalb eines Abschnitts = Empfehlung.

Stand: 16.10.2026 (abends)

## Jetzt: Phase 2 – die App (Tauri 2 + Leptos)

- [x] Gerüst `apps/app/` (Start: `cd apps/app && cargo tauri dev`)
- [x] Splinely einbinden: `.sply` → interaktives SVG über `splinely_document` v0.5.2, sandboxed `<Sply>`-Komponente
- [x] Aufdecken: Bild zusätzlich direkt am Locus auf der Bühne zeigen; `image_size` über CLI/Quelldatei einstellbar (Standard 0,25)
- [x] Editor-Grundschnitt: Loci setzen, ziehen/Pfeiltasten, Inhalte und Antworten inline mit Autosave, Bildgröße per Ziehgriff/Zahlenfeld, Löschen
- [ ] Visuelle WebView-/E2E-Prüfung von Ortsbildern, Pointer-Gesten und Autosave-Fokus (`docs/app-editor-testplan.md`)
- [x] „Weiter“-Zeile mit fälligen Loci; Einstellungen (neue Loci pro Tag, Behaltensrate) und Statistik (⚙ auf dem Startbildschirm)
- [x] Import/Export von `.locora` in der App (Dateidialoge, mit Wiederholungsstand)
- [x] Plugin-Kacheln (`ui.tile`) auf dem Startbildschirm; letzte Kachel je Plugin bleibt im Profil gespeichert
- [x] Befehlspalette (Strg/⌘+K): Paläste öffnen/üben, Weiter, Übersicht, Einstellungen; blockiert bei ungespeicherten Editor-Änderungen
- [x] Kachel des Plugins `review`: Loci im Plan und Datum der nächsten Fälligkeit (veraltet nicht)
- [x] Kachel-Icons als `.sply` (`assets/ui`)
- [x] Core-Ansicht: Palastliste mit ▶ auf der Karte, Raum mit Hintergrund und Locus-Markern, Bild am Ort
- [x] Journey in der App: Recall (Leertaste, 1–4), Quiz, Guess; gleiche Bewertung über das Core-Plugin
- [x] Aufdecken zeigt das `.sply` (es gehört zur Antwort), vorher nur Hintergrund und Marker
- [x] Workbook in der App: Eingaben vor der Journey abfragen, Aktionen als Knöpfe
- [x] Workbook-Eingaben passend zum Typ (Zahlenfeld, Ja/Nein-Kästchen)
- [x] Workbook-Aktionen auch nach dem Bewerten (wie `locora practice`), auch nach der letzten Frage
- [x] Liste = Route: Strg/⌘+V auf der fokussierten Bühne oder Textfeld/Knopf im Editor, 1–100 Begriffe, Schlangenroute, atomarer Batch mit Validierung und Op-Log (keine Video-Räume)
- [ ] Liste = Route: WebView-/E2E-Prüfung und Klickbudget nachweisen
- [x] Locus-Mediendatei-Auswahl: nativer Dialog, Bild wählen/entfernen, Raster-MIME-/`.sply`-Validierung, Scope-Prüfung und Fehleranzeige ohne freie Pfad-IPC (64 MiB)
- [ ] Nativen Locus-Dateipicker im WebView manuell prüfen (Auswahl, Abbruch, Fehler, Entfernen, Neustart); mobile URI-Auswahl noch nicht unterstützt
- [x] Startknopf „Bild/Video wählen…“: nativer Dialog, gemeinsame Signaturprüfung (64 MiB), atomarer neuer Palast + passender Raum + Medienmetadaten/Op-Log, Dateiname mit Kollisionssuffix, direkt Editor öffnen; Zurück aktualisiert Home
- [ ] Start-Dateipicker im WebView manuell prüfen (Bild/Video, Abbruch, Fehler, Editor, Zurück, Neustart); lokale Dateien, keine mobilen Content-URIs
- [ ] Editor-Ausbau: Bild/Video hineinziehen, Video-Loci mit Zeit, Undo/Redo und Entwurfswiederherstellung
- [x] Tutorial in der App: Erststart mit Simo, „Tutorial starten“ legt den Palast an und öffnet ihn (Animationen über die Runtime)
- [x] Locus-Marker als eigene `.sply` (neu/fällig/gewusst), pro Palast austauschbar; Hintergründe ohne Loci
- [x] Marker in der App an jede Locus-Position setzen und Zustände über die Runtime umschalten
- [ ] Viele Marker: in einem SVG pro Raum zusammenfassen statt je ein Frame
- [x] Grafiken als `.sply` (Stil `assets/STIL.md`): Kachel-Icons, Journey-Feedback (richtig/nochmal/geschafft), Willkommen mit Simo – `assets/ui/src/ui.py`, Simo aus `assets/mascot/src/simo.py`
- [ ] Weitere Grafiken: Raum-Arten (ohne Bild, Bild, Video), „nichts fällig“ (Simo mit Laterne), Splash/Ladezustand
- [ ] Android- und iOS-Builds in CI; Klick-Budgets (PLAN 6.3) als E2E-Tests
- [x] Systemvoraussetzungen für Linux-Builds dokumentiert (`apps/app/README.md`)

## Antworten und Abfragen (aus dem Abgleich mit mnemodim)

- [x] Quiz mischen und Ablenker zufällig aus einem Pool (bis 30 Optionen) ziehen
- [x] Guess: weitere gültige Antworten (`accept`) und tolerantere Prüfung
- [x] Merkhilfe `note` pro Locus, beim Aufdecken gezeigt
- [x] Abfragerichtungen: `--reverse`, `--numbers`, `--shuffle`
- [x] Dieselben Übungsarten und die Merkhilfe in der App (Schalter in der Palastansicht)
- [ ] Später: Ton beim Aufdecken (in mnemodim `soundPath`), über Audio in Splinely (`audioAssets`/`playAudio` im `.sply`)
- [ ] Zoom/Schwenk pro Locus in der App (mnemodim `zoom/panX/panY`)

## Phase 1 – Reste

- [ ] Bilder an Orten im Palast-Dienst `core.palaces@1` (bisher nur Store, Format, `locora.toml`, CLI)
- [ ] Sammlungen: mehrere Paläste mit gemeinsamer Navigation und `navigate`-Aktion (erst wenn gebraucht)
- [x] FSRS-Optimierung in der App (Einstellungen) mit Hinweis beim ersten Mal ab 100 und dann je 1 000 neuen Wiederholungen; Tipp auch in `locora stats`
- [x] Lerntage in lokaler Zeit: Tagesbeginn einstellbar (Standard 4:00), UTC-Versatz setzt die App bei jedem Start (CLI: `settings day-start`, `settings utc-offset`)
- [ ] Plugin-Signaturen (Ed25519), Update-Befehl für Plugins

## Phase 3 – Video-Räume in der App und Benachrichtigungen

- [ ] Video-Räume: anhalten am Locus bis gelöst, Editor mit Taste L, Zeitleiste, Start-Vorschau aller Loci
- [ ] Plugin `notify`: lokale Benachrichtigungen, Ruhezeiten, Bündelung, Tipp öffnet die passende Stelle

## Phase 4 – Tutorial-Plugin

- [x] Tutorial-Palast „Locora lernen“ mit `.sply` an jedem Ort (`locora tutorial`)
- [ ] Touren durch die App über `ui.highlight@1`, Simo als Begleiter
- [ ] Formel-Guide als Palast mit `.sply`-Bildern

## Phase 5 – Gamification und Locora Cloud

- [ ] Plugin `gamification` (lokal, kostenlos): XP, Combos, Herzen, Trophäen, Daily Challenges, Streak – Grafiken als `.sply`
- [ ] Privates Repo `locora-cloud` (axum + Postgres): Abo, Ranglisten, Freunde, gemeinsame Challenges
- [ ] Ed25519-Entitlements mit 14 Tagen Offline-Kulanz; In-App-Kauf auf iOS/Android, Merchant of Record auf Desktop

## Phase 6 und 7

- [ ] Sync über das Op-Log (HLC, Last-Writer-Wins pro Feld) mit Locora Cloud
- [ ] Plugin-Verzeichnis, WASM-Plugins von Dritten mit deklarativer UI, Store-Prüfung für iOS/Android

## Entscheidungen, die noch anstehen

- [x] Splinely-Release 0.5.2 lokal installiert; alle Locora-Grafiken bestehen `check` ohne Hinweise
- [x] Repo privat; PRs und CI-Status über `gh` mit Token. Actions-Kontingent: CI nur noch auf Linux, macOS/Windows per Hand (`workflow_dispatch`)
