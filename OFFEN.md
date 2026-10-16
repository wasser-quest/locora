# Offen

Alles, was noch gebaut oder entschieden werden muss. Wird bei jedem Schritt aktualisiert. Erledigtes wird hier gestrichen und in `PLAN.md` (Abschnitt 13) bzw. den Commits festgehalten. Reihenfolge innerhalb eines Abschnitts = Empfehlung.

Stand: 16.10.2026

## Jetzt: Phase 2 – die App (Tauri 2 + Leptos)

- [x] Gerüst `apps/app/` (Start: `cd apps/app && cargo tauri dev`)
- [x] Splinely einbinden: `.sply` → interaktives SVG über `splinely_document` v0.5.2, sandboxed `<Sply>`-Komponente
- [ ] Aufdecken: Bild zusätzlich direkt am Locus auf der Bühne zeigen, Größe pro Locus einstellbar (neues Feld `image_size`)
- [ ] Startbildschirm: Plugin-Kacheln (`ui.tile`), „Weiter“-Zeile mit fälligen Loci, Befehlspalette (Strg/⌘+K)
- [ ] Core-Ansicht: Palastliste mit ▶ auf der Karte, Raum mit Hintergrund und Locus-Markern, Bild am Ort
- [x] Journey in der App: Recall (Leertaste, 1–4), Quiz, Guess; gleiche Bewertung über das Core-Plugin
- [x] Aufdecken zeigt das `.sply` (es gehört zur Antwort), vorher nur Hintergrund und Marker
- [ ] Workbook in der App: Eingaben abfragen, Aktionen als Knöpfe
- [ ] Editor: Bild/Video ins Fenster ziehen = neuer Palast, Klick = Locus, eingefügte Liste = Loci als Route
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
