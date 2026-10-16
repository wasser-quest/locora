# Offen

Alles, was noch gebaut oder entschieden werden muss. Wird bei jedem Schritt aktualisiert. Erledigtes wird hier gestrichen und in `PLAN.md` (Abschnitt 13) bzw. den Commits festgehalten. Reihenfolge innerhalb eines Abschnitts = Empfehlung.

Stand: 16.10.2026

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
- [ ] Eigene Kacheln der offiziellen Plugins (z. B. `review`: fällig heute) und Kachel-Icons als `.sply`
- [ ] Core-Ansicht: Palastliste mit ▶ auf der Karte, Raum mit Hintergrund und Locus-Markern, Bild am Ort
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
- [ ] Tutorial in der App: `locora tutorial`-Palast mit abgespielten Animationen; Erststart-Hinweis
- [x] Locus-Marker als eigene `.sply` (neu/fällig/gewusst), pro Palast austauschbar; Hintergründe ohne Loci
- [x] Marker in der App an jede Locus-Position setzen und Zustände über die Runtime umschalten
- [ ] Viele Marker: in einem SVG pro Raum zusammenfassen statt je ein Frame
- [ ] Grafiken als `.sply` (Stil `assets/STIL.md`): Kachel-Icons für core/review/notify/tutorial/gamification, Raum-Arten, leere Zustände (Simo mit Laterne), Journey-Feedback, Splash
- [ ] Android- und iOS-Builds in CI; Klick-Budgets (PLAN 6.3) als E2E-Tests
- [ ] Systemvoraussetzungen für Linux-Builds dokumentieren (webkit2gtk-4.1, gtk3, libsoup3 Entwicklerpakete)

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
- [ ] Automatische FSRS-Optimierung (z. B. Hinweis nach je 1 000 neuen Wiederholungen); Tageswechsel derzeit UTC – lokale Zeitzone/Tagesbeginn einstellbar machen
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

- [ ] Splinely-Release 0.5.2 lokal installieren (behebt den falschen `invisible`-Hinweis von `check`)
- [ ] Repo-Sichtbarkeit: Die GitHub-API meldet das Repo als nicht gefunden (vermutlich privat) – CI-Status nur über die Weboberfläche oder mit Token sichtbar
