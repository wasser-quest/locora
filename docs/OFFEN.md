# Offen

Alles, was noch gebaut oder entschieden werden muss. Wird bei jedem Schritt aktualisiert. Erledigtes wird hier gestrichen und in `PLAN.md` (Abschnitt 13) bzw. den Commits festgehalten. Reihenfolge innerhalb eines Abschnitts = Empfehlung.

Stand: 22.10.2026

## Jetzt: Phase 2 – die App (Tauri 2 + Leptos)

- [x] Gerüst `apps/app/` (Start: `cd apps/app && cargo tauri dev`)
- [x] Splinely einbinden: `.sply` → interaktives SVG über `splinely_document` v0.5.2, sandboxed `<Sply>`-Komponente
- [x] Aufdecken: Bild zusätzlich direkt am Locus auf der Bühne zeigen; `image_size` über CLI/Quelldatei einstellbar (Standard 0,25)
- [x] Editor-Grundschnitt: Loci setzen, ziehen/Pfeiltasten, Inhalte und Antworten inline mit Autosave, Bildgröße per Ziehgriff/Zahlenfeld, Löschen
- [x] Figurenklicks per Bridge: SVG-Hit-Test und Splinely-Interaktionen, App-Controls/Editor ausgenommen; Simos Gesichts-Klick startet `hop` im REA/WebKit-Test
- [x] Cursorposition per Bridge an Splinely weiterleiten, ohne Klicks/Fokus abzufangen; Simo-Pupillenbewegung mit REA/WebKit geprüft
- [x] Splinely-Startprüfung pro Frame: Runtime-Bestätigung per Nachricht, sichtbarer Fehler bei fehlendem Start; expliziter Bootstrap als Fallback
- [x] Frischen Debug-Build mit separatem Profil nativ geprüft: Willkommens-Simo und Simo am ersten aufgedeckten Tutorial-Locus bewegen sich; REA-Prozessbeleg und Grenzen in `docs/splinely-runtime-smoke.md`
- [ ] Gemeldeten statischen Zustand im betroffenen Build reproduzieren; WebView-Release-Build, alle Tutorial-Loci und Reduced Motion sichtbar prüfen (UI-Release-Bundle baut, Ursache noch nicht bestätigt)
- [x] Nativer WebKitGTK-Smoke-Test `apps/app/tests/native_smoke.py` (AT-SPI + xdotool, eigenes Profil): Tutorial, Journey-Start (1 Klick), Recall per Tastatur, Autosave, Tastatur-Verschieben + Undo/Redo persistent, Liste einfügen (0 Mausklicks) bestanden
- [x] Smoke-Test: Datei-Drop per xdotool-Harness kommt im Fenster an (Palast, Raum und Editor; Videoraum); nativ bestanden in `apps/app/tests/native_smoke.py`
- [ ] Visuelle WebView-Prüfung von Ortsbildern und Pointer-Gesten (`docs/app-editor-testplan.md`)
- [x] „Weiter“-Zeile mit fälligen Loci; Einstellungen (neue Loci pro Tag, Behaltensrate) und Statistik (⚙ auf dem Startbildschirm)
- [x] Import/Export von `.locora` in der App (Dateidialoge, mit Wiederholungsstand)
- [x] `locora.toml`-Import auf Desktop: relative Medien, neue Kopien bei Namenskonflikten, Validierung vor dem Bauen; Mobil importiert `.locora`-Archive
- [x] TOML-Export (`dump`) in der App: nativer Ordnerdialog, Quelle mit Medien, kein Überschreiben, Roundtrip-Test
- [ ] TOML-Import/-Export im nativen Dialog prüfen (Auswahl, Abbruch, Medien, Fehler)
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
- [x] Locus-Mediendatei-Auswahl: nativer Dialog, Bild wählen/entfernen, Raster-MIME-/`.sply`-Validierung, Scope-Prüfung und Fehleranzeige ohne freie Pfad-IPC (64 MiB)
- [ ] Nativen Locus-Dateipicker im WebView manuell prüfen (Auswahl, Abbruch, Fehler, Entfernen, Neustart); mobile URI-Auswahl implementiert, Gerätetest offen
- [x] Startknopf „Bild/Video wählen…“: nativer Dialog, gemeinsame Signaturprüfung (64 MiB), atomarer neuer Palast + passender Raum + Medienmetadaten/Op-Log, Dateiname mit Kollisionssuffix, direkt Editor öffnen; Zurück aktualisiert Home
- [ ] Start-Dateipicker im WebView manuell prüfen (Bild/Video, Abbruch, Fehler, Editor, Zurück, Neustart); mobile Content-URIs implementiert, Gerätetest offen
- [x] Video-Loci im App-Editor über Sekundenangabe anlegen; validiert und persistent
- [x] Videozeit aus Wiedergabe übernehmen: Knopf und Taste L auf der fokussierten Bühne, pausiert vor dem Anlegen
- [x] Video-Ortsnavigation im Editor: chronologische Zeitknöpfe, pausieren/spulen/auswählen, aktive Auswahl
- [x] Video-Editor im WebView prüfen: Taste L/Fokus, Zeitknopf-Navigation (chronologisch, pausiert/spult/auswählt) und Persistenz nativ getestet; ungeladenes Video (Statusmeldung) und wiederholte Eingabe manuell prüfen
- [x] Video-Zeitleiste im Editor zeigt Ortszeiten proportional zur geladenen Videodauer; Orte hinter dem Videoende sind markiert und können nicht angesprungen werden
- [x] Freier Suchregler in Video- und Splinely-Zeitleiste; Bühnenklick setzt einen zeitgebundenen Ort an der angeklickten Position; Marker erscheinen erst im eingestellten Zeitfenster
- [x] Freier Suchregler und Bühnenklick platzieren Video- und animierte `.sply`-Orte im nativen WebView an Zeit und Bildposition (`native_smoke.py`, Stickfight-Fixture)
- [x] `.sply`-Marker im nativen WebView vor dem Locus-Zeitpunkt verborgen und am Zeitpunkt sichtbar (`--only-sply`)
- [ ] Video-Markerfenster und pro-Ort-Vorlauf im nativen WebView sichtbar prüfen
- [x] Editor-Ausbau: Datei hineinziehen über einmalige Tickets (Startseite → neuer Palast, Editor → Ortsbild); Undo/Redo (Knöpfe, Strg/⌘+Z/Y) mit Backend-Belegen für Hinzufügen/Einfügen/Löschen (Konfliktprüfung, neueste 500); Entwurfswiederherstellung über `localStorage`
- [x] Tutorial in der App: Erststart mit Simo, „Tutorial starten“ legt den Palast an und öffnet ihn (Animationen über die Runtime)
- [x] App-Tour: Zurück-Knopf; aufeinanderfolgende Schritte derselben Ansicht lassen die Ansicht und Simo geladen, statt sie neu aufzubauen
- [x] App-Tour im nativen WebView: alle sechs Schritte vorwärts und rückwärts sowie Schließen geprüft (`native_smoke.py --only-tour`)
- [ ] App-Tour auf sichtbares Flackern und erhaltenen Journey-Stand bei Routenwechseln prüfen
- [x] Locus-Anzeige auf diesem Gerät einstellbar: vorherige/nächste Route-Orte (0–10), Nummern direkt im Markerpunkt, aktuellen Marker nach Aufdecken ausblenden, Video-Vorlauf global und pro Ort
- [x] Locus-Marker als eigene `.sply` (neu/fällig/gewusst), pro Palast austauschbar; Hintergründe ohne Loci
- [x] Marker in der App an jede Locus-Position setzen und Zustände über die Runtime umschalten
- [x] Medien über HTTP-Range ausliefern: Einzelbereiche, offene/Suffix-Bereiche, 416 und HEAD; nur angeforderte Bytes lesen
- [x] Video-Spulen: Videos laufen als Blob (WebKitGTK lehnt Custom-Schemes als Medienquelle ab); Spulen bleibt in der WebView, Range-Anfragen des Backends sind backend-getestet
- [x] Viele Marker: ein gemeinsamer Frame pro Raum (`/markers/<hash>`, `marker_layer.js`), eine Runtime, IDs je Klon umbenannt; Prüfskript `apps/app/tests/marker_layer_probe.py`
- [ ] Marker-Layer in der App sichtbar prüfen (Zustände, Ziehen, Pupillen/Pointer, viele Loci)
- [x] Grafiken als `.sply` (Stil `assets/STIL.md`): Kachel-Icons, Journey-Feedback (richtig/nochmal/geschafft), Willkommen mit Simo – `assets/ui/src/ui.py`, Simo aus `assets/mascot/src/simo.py`
- [x] Weitere Grafiken: Raum-Arten in den Tabs, „nichts fällig“ mit Simo, Ladezustand mit dem Locora-Logo
- [x] Mobile-CI `.github/workflows/mobile.yml`: Android-Debug-APK bei Push/PR, iOS-Simulator-App nur per Hand (macOS-Kontingent); TOML-Ordnerexport auf Mobil mit klarer Fehlermeldung
- [ ] iOS: Signierung (Apple-Entwicklerkonto) und Gerätetest; Releases enthalten bisher ein unsigniertes `.ipa`, iOS verwendet Pulley statt JIT
- [ ] Mobile Befehlspalette per Abwärtswisch vom oberen Rand und sichtbarem „Befehle“-Knopf öffnen; WASM-Check und Gestenlogik-Test bestanden; Abwärtswisch auf dem Gerät noch offen (Knopf ist geprüft, siehe nächster Punkt)
- [x] Android-Gerätesmoke-Test (Nothing A059P, Android 16, ARM64-Debug-APK aus `b11aad6`, 19.10.2026): Installation über die bestehende App ohne Datenverlust, Startseite mit Bestandsdaten, „Befehle“-Knopf öffnet die Palette mit Tastatur, Palastansicht öffnet sich per Karte; Logcat ohne Abstürze/ANRs. Journey-Ansicht rendert (Fortschritt, Herzen, Raumbild mit Marker; ohne Antwort). Bewertung auf dem Tutorial-Palast „Locora lernen“ geprüft (eine Frage, „Gut“): Combo und Herzen erscheinen, Weiter-Knopf bereit. Nicht geprüft: Marker-Layer, Video-Wiedergabe, Dateipicker. Abwärtswisch per `adb input swipe` nicht ausgelöst (Systemleiste fängt den oberen Rand). Beobachtung, nicht reproduziert: Android-Zurück aus der Journey hat die App einmal beendet
- [x] Einstellung „Direkt in Core starten“ (PLAN 6.1): Gerätepräferenz, direkte Palastübersicht und wechselbares Raumvorschaubild auf Karten (Video mit Raum-Icon statt geladenem Film)
- [x] Core-Startpräferenz und Bildwechsel nach App-Neustart im nativen WebView geprüft (`native_smoke.py --only-core`); `LOCORA_DATA` isoliert jetzt auch den WebKit-Speicher
- [x] Restliche Klick-Budgets (PLAN 6.3) als E2E-Tests: alle sechs Budgets laufen im nativen Smoke-Test
- [x] Systemvoraussetzungen für Linux-Builds dokumentiert (`apps/app/README.md`)

## Antworten und Abfragen (aus dem Abgleich mit mnemodim)

- [x] Quiz mischen und Ablenker zufällig aus einem Pool (bis 30 Optionen) ziehen
- [x] Guess: weitere gültige Antworten (`accept`) und tolerantere Prüfung
- [x] Merkhilfe `note` pro Locus, beim Aufdecken gezeigt
- [x] Abfragerichtungen: `--reverse`, `--numbers`, `--shuffle`
- [x] Dieselben Übungsarten und die Merkhilfe in der App (Schalter in der Palastansicht)
- [x] Ton beim Aufdecken: eigener Tonclip pro Locus (`audio`, `.locora` Format 4/5, `locora.toml`, CLI `--audio`, Editor, Sync); Audio in Splinely selbst ist nicht nötig
- [x] Zoom pro Locus (1 bis 4): beim Abfragen zoomt die Bühne auf den Ort, der Ausschnitt bleibt im Bild (Schwenk ergibt sich aus der Position); Store, `.locora` (Format 3), `locora.toml`, CLI `--zoom`, Palast-Dienst, Editor

## Phase 1 – Reste

- [x] Bilder an Orten im Palast-Dienst `core.palaces@1` (`image`, `image_size`; Plugin-API 0.6)
- [ ] Sammlungen: mehrere Paläste mit gemeinsamer Navigation und `navigate`-Aktion (erst wenn gebraucht)
- [x] FSRS-Optimierung in der App (Einstellungen) mit Hinweis beim ersten Mal ab 100 und dann je 1 000 neuen Wiederholungen; Tipp auch in `locora stats`
- [x] Lerntage in lokaler Zeit: Tagesbeginn einstellbar (Standard 4:00), UTC-Versatz setzt die App bei jedem Start (CLI: `settings day-start`, `settings utc-offset`)
- [x] Plugin-Zustimmung an Manifest und Hash der tatsächlich installierten WASM-Datei binden; alte Belege erfordern Neuinstallation
- [x] Plugin-Signaturen (Ed25519) und Update-Befehl: `locora plugin keygen`, `sign` (`<datei>.sig`), `trust`, `publishers`, `update` (nur neuere Version desselben Herausgebers); Installation zeigt den Herausgeber, Approvals speichern ihn (`locora-signing`)
- [x] `.locora-plugin`-Paketformat: `locora plugin pack`, `check`, `install` und `update` mit begrenzten ZIP-Einträgen, Manifestabgleich und Signaturprüfung; rohe WASM-Dateien bleiben unterstützt
- [x] Plugin-Vorlagen `befehl`, `locus-typ`, `importer` als aufrufbare Dienst-Beispiele mit `locora plugin invoke`; `locus-typ` ist noch kein automatisch gerenderter Journey-Typ
- [ ] Mobiles `plugin dev --device` braucht App-Gegenstelle für Pairing, authentisierten Transfer, Rechtebestätigung und Hot-Reload (PLAN 4.4.1)

## Phase 3 – Video-Räume in der App und Benachrichtigungen

- [x] Echte Animationsdateien backend-geprüft: `~/tester/cyber-stickfight/cyber-fight.gif` (252 Frames, 12,6 s), `~/tester/stickfight/fight.sply` und testweise MP4-Konvertierung; optionaler Fixture-Test ohne Dateien im Repo
- [x] `.sply`-Animationsräume mit Zeitsteuerung über die Splinely-Runtime in Editor und Journey; Editor-Suchregler und Platzierung mit Stickfight-Fixture nativ bestanden, Journey-Sichtprüfung offen
- [ ] GIF-Animationsräume brauchen steuerbare Wiedergabe statt `<img>`
- [x] Editor: Frame-Schritte per , und . auf der fokussierten Bühne, Positionsanzeige; nativ geprüft (`video-frame-steps`, 0,03 s je Schritt)
- [x] Video-Journey: Vorschau aller Orte bei Zeit 0 (PLAN 7.2); Editor: Zeitpunkte per Regler ziehen (`Retime`-Aktion mit Raum- und Zeitvalidierung)
- [ ] Visuelle WebView-Prüfung der Stickfight-Animationen (Backend-Test beweist keine Wiedergabe)

- [x] Video-Journey: Locus-Zeit im IPC; stumm bis zum Locus abspielen, dort bis Bewertung halten; Antworten während der Wiedergabe sperren; Autoplay-Fallback und Fehleranzeige
- [x] Video-Journey nativ geprüft (2 Loci, Rückwärtsrichtung, Autoplay, Tastatur durch alle Phasen); 16-ms-Prüftimer endet nach Erreichen des Locus
- [x] Video-Journey: Vorschau aller Orte bei Zeit 0 und Frame-Prüfung über `requestVideoFrameCallback` mit `timeupdate`-/Timer-Fallback; UI-Release-Bundle baut, native Sichtprüfung steht aus
- [x] Plugin `notify`: lokale Benachrichtigungen, Ruhezeiten, Bündelung, Tipp öffnet die passende Stelle (send/schedule/due/configure/history; App-Toasts mit Aktion `review.start@…`, CLI `locora notify ls|show|send`; plugin→plugin-Aufrufe wie review→notify bleiben ein Host-Thema für Phase 5)

## Phase 4 – Tutorial-Plugin

- [x] Tutorial-Palast „Locora lernen“ mit `.sply` an jedem Ort (`locora tutorial`)
- [x] Erste App-Tour: offizielles `tutorial`-Plugin, geprüfter Host-Dienst `ui.highlight@1`, drei Startseiten-Schritte mit Simo, Weiter/Schließen und gespeichertem Schrittstand
- [x] Tour auf Palastansicht, Editor und Journey ausgedehnt
- [x] Tour stoppt bei fehlenden DOM-Zielen oder Routenfehlern, bietet erneuten Versuch und führt den Fokus zum Ziel bzw. nach dem Schließen zurück
- [ ] Tour: fehlende Ziele und Fokusführung nativ prüfen
- [x] Formel-Guide „Formeln verstehen“ als eigener Palast mit sechs `.sply`-Orten und ausführbaren Beispiel-Bindungen (`locora formula-guide` und App)

## Phase 5 – Gamification und Locora Cloud

- [x] Plugin `gamification` (lokal, kostenlos): XP, Combos, Herzen, vier Trophäen, Tagesziel und Streak; Status in CLI und App-Kachel
- [x] Herzen und Combo während der Journey zeigen: Rückmeldung nach jeder Antwort (`Feedback.game`); nativ geprüft (`recall-keyboard-reveal-and-grade`)
- [x] Combo, Herzen und Trophäen als `.sply`-Grafiken in der Journey darstellen
- [x] Lernserie mit eigener `.sply`-Grafik in der Journey darstellen
- [x] Lernserie auch in der Gamification-Kachel auf der Startseite darstellen
- [x] Neue Spielregeln (Plugin 0.2.0, PLAN 4.7): XP für fällige Wiederholungen (10) und neue Orte (6) pro Ort und Lerntag, egal wie bewertet; früheres Üben bringt nichts; keine Herzen; Combo nur bei Quiz und Guess; Tagesziel = das Fällige (ab 50 bei Rückstand erreicht); selbst gewähltes Wochenziel statt Tagesserie; Trophäen mit Datum; Palastmedaillen aus der FSRS-Stabilität mit eigenen `.sply`-Medaillen; Abschlusskarte mit XP und nächster Wiederholung; einmalige Erklärung in der Journey; „Fortschritt“-Seite in allen Sprachen; keine Benachrichtigungen aus der Gamification
- [x] Privates Repo `locora-cloud` (axum + Postgres, `wasser-quest/locora-cloud`): Konten, Sitzungen, Ranglisten, Freunde, Challenges, Sync, Medien; Tests gegen PostgreSQL
- [ ] Bezahlen über Paddle (Merchant of Record), **nur im Web**: 5 €/Monat oder 48 €/Jahr; nur bestätigte Adressen kaufen, die Konto-ID geht durch den Checkout. Kein In-App-Kauf: die nativen Apps zeigen nie Preise, Kaufknöpfe oder Links zu einer Kasse (Store-Regeln gegen Steering); `apps/app/ui/src/store_rules.rs` prüft das in den Tests. Der Webhook des Servers ist anbieterneutral; Konto und Vertrag bei Paddle fehlen
- [x] Ed25519-Entitlements mit 14 Tagen Offline-Kulanz: der Server stellt sie aus, `locora cloud status` prüft sie ohne Netz

## Phase 6 und 7

- [x] Sync über das Op-Log (HLC, Last-Writer-Wins pro Feld, Tombstones) mit Locora Cloud: `locora cloud sync`, Medien per Hash, Journeys werden auf dem Server gewertet
  - Erledigt: Cloud-Anmeldung/Registrierung, Schlüsseleinrichtung mit einmalig gezeigtem Wiederherstellungsschlüssel (als Datei speichern oder kopieren) und manueller verschlüsselter Sync in den App-Einstellungen, auch im Browser
  - Erledigt: schnellerer Sync (eine Verbindung, Medien gesammelt prüfen, vier Übertragungen gleichzeitig, Pakete nach Größe, Ausgang zusammengefasst), Übersicht vor dem Sync nach Palast, Bestätigung vor dem Senden einer Palast-Löschung, Auto-Sync mit dem Schlüssel im Schlüsselspeicher des Systems bzw. WebCrypto im Browser; siehe [`CLOUD.md`](CLOUD.md)
  - Erledigt: Gastkonten synchronisieren im sechsmonatigen Testzeitraum ohne signierte Freischaltung
  - Offen: realer Durchlauf gegen den neuen Server und mit echten Geräten (Desktop, Android, iPhone, Browser), auch Auto-Sync
  - Offen: Wiederherstellen mit dem Wiederherstellungsschlüssel gibt es nur in der CLI (`locora cloud encrypt recover`), nicht in der App
  - Offen: Android hält den Auto-Sync-Schlüssel über den Android Keystore verschlüsselt in den privaten Einstellungen der App (`android-native-keyring-store`); auf einem echten Gerät prüfen, auch nach Neuinstallation und Sicherung
  - Erledigt für neu gebaute Paläste: Tutorial und Formel-Guide erhalten geräteübergreifend feste Kennungen für Palast, Räume und Loci. Bereits vorhandene Profile brauchen einen bewusst ausgelösten Neuaufbau; dieser setzt ihren bisherigen Wiederholungsstand zurück.
  - Erledigt: Gamification-Fortschritt wird verlustfrei zusammengeführt: Fakten pro Gerät unter eigenen Schlüsseln, aus denen XP, Lerntage, Wochen und Trophäen abgeleitet werden (`storage.list@1`, `core.journey.reviewed`); der alte Stand wird einmal pro Gerät übernommen (größter Wert, nie die Summe); im Browser liegt der Spielstand jetzt im Profil des Kerns und wird mitsynchronisiert (vorher nur in IndexedDB). Ältere App-Versionen zeigen bis zum Update ihre alten Zahlen.
  - Erledigt: „Spielfortschritt löschen“ (Einstellungen, `locora game reset`): neue Epoche, die gegen später eintreffende alte Fakten gewinnt; auf allen Geräten geleert; Ranglistenpunkte werden auf dem Server gelöscht (`DELETE /v1/game/data`, bei fehlender Antwort beim nächsten Sync erneut)
  - Offen: Der Wiederholungsplan im Browser liegt weiter nur in IndexedDB und wird nicht synchronisiert (wie zuvor); dafür müsste auch `review` seinen Speicher im Kern führen
  - Offen: Karten und Antwortlog von `review` sind je Palast ein LWW-Wert (`cards/<palast>`, `log/<palast>/<n>`); lernen zwei Geräte offline im selben Palast, gewinnt ein Stand. Braucht dasselbe Faktenmodell wie die Gamification
  - Offen: eine Wiederholungsrunde für heute falsch Beantwortetes („relearn“) und ein Coach für immer wieder vergessene Orte („leech“)
- [ ] Plugin-Verzeichnis, WASM-Plugins von Dritten mit deklarativer UI, Store-Prüfung für iOS/Android

## Locora Cloud: als Nächstes

Reihenfolge: Deploy → Rangliste → Bezahlen.

- [x] Passwort zurücksetzen per E-Mail (Link 1 h, „Passwort vergessen?“ in der App führt zu `web.locora.cloud/forgot`)
- [x] E-Mail-Bestätigung nach der Registrierung, mit „Link erneut senden“ und „Ich habe bestätigt – prüfen“ in der App
- [x] Server gehärtet: Speicherkontingente pro Konto (Standard Medien 512 MiB Gast / 5 GiB mit Abo, Änderungen 256 MiB / 2 GiB), serverweite Budgets, Zeitlimits, Rate-Limits, Wartungs-Job (Warnung vor dem Löschen, Aufräumen), tägliches verschlüsseltes Datenbank-Backup; die App meldet volle Speicher (507), Überlast (503) und 429 neutral, ohne Abo-Hinweis
- [x] Kontoseite `web.locora.cloud` (statisch, im Server-Repo unter `web/`): Konto, Speicher, Passwort, Abmelden, Daten oder Konto löschen, Mail-Links
- [ ] Neuen Server und die Kontoseite deployen (vorher fragen); danach den Gerätedurchlauf oben
- [ ] Rangliste neu: eine öffentliche Rangliste statt Freunden und Challenges, Zeiträume Tag/Woche/Monat/Jahr, Teilnahme nur auf Wunsch mit Anzeigename, kostenlos. „Melden“ (Schummeln, unpassender Name), automatisch ausgeblendet ab 5 Meldungen verschiedener bestätigter Konten; Ausblenden pro Gerät; Server-Befehle für den Betreiber. In der App als abschaltbare Kachel/Seite. Freunde und Challenges aus App, CLI und Server entfernen
- [ ] Bezahlen über Paddle (siehe Phase 5), nur im Web
- [ ] Rechtliches für das Abo: AGB, Widerrufsbelehrung, Preisseite, Datenschutzerklärung ergänzen
- [ ] Support-Adresse einrichten und nennen
- [ ] Sicherung der Medien im S3-Bucket (die Datenbank wird schon gesichert)
- [ ] Datenexport (DSGVO)
- [ ] Seite „Mnemotechnik lernen“ (in Arbeit)

## Entscheidungen, die noch anstehen

- [x] Betrieb von Locora Cloud: Hosting, TLS-Proxy (Caddy), Sicherung der Datenbank, Konto löschen, Passwort zurücksetzen; siehe `locora-cloud/README.md` und `locora-cloud/deploy/BETRIEB.md`
- [x] Angemeldete Cloud-Passwortänderung (`PUT /v1/me/password`, `locora cloud password`) mit altem Passwort, 10 Versuchen je Stunde und Widerruf aller Sitzungen
- [x] Cloud-Medienuploads pro Konto auf 200 neue Versuche je Stunde begrenzen; fertige Uploads lassen sich ohne weiteres Kontingent wiederholen (atomarer PostgreSQL-Zähler)
- [ ] Cloud-Integrationstest für das Upload-Limit gegen PostgreSQL ausführen (Test kompiliert; hier ist derzeit keine Testdatenbank erreichbar)
- [ ] Schlüsselverwaltung: Entitlement-Schlüssel und Webhook-Geheimnis liegen lokal in `~/.config/locora-cloud` (nicht im Repo); vor dem Betrieb in einen Secret-Store verlegen und Rotation planen
- [x] Splinely-Release 0.5.2 lokal installiert; alle Locora-Grafiken bestehen `check` ohne Hinweise
- [x] Repo öffentlich; CI prüft Pull Requests auf Linux, Pushes auf `main` auch auf macOS und Windows
