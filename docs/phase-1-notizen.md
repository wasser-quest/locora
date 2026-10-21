# Phase 1: Umsetzungsnotizen

Ausführliche Verträge, Randfälle und Grenzen der Phase-1-Schritte. Die Kurzfassung steht in [`PLAN.md`](PLAN.md), Abschnitt 13.3.

## Schritte

1. **Workbook in der CLI – erledigt:**
   - `locora table show [PALAST]` zeigt effektive Tabellen; ohne Palast oder mit `--shared` nur gemeinsame Tabellen
     (`PALAST` und `--shared` schließen sich aus).
   - `locora table set (--palace P | --shared) Tabelle.key[.spalte] WERT [-t typ]`, Standardspalte `value`,
     Standardtyp `text`; Typen: `text/number/boolean/json/formula/asset/palace/room`.
     Änderungen erhalten andere Spalten derselben gespeicherten Zeile. Lokale Zeilen ersetzen gemeinsame **vollständig**;
     beim ersten lokalen Set werden keine gemeinsamen Spalten kopiert.
   - `locora table rm (--palace P | --shared) Tabelle.key[.spalte]` löscht eine Zeile oder einzelne Zelle;
     nach Löschen der letzten lokalen Zelle greift wieder die gemeinsame Zeile. Punktgetrennte Adressen unterstützen
     keine Punkte in Zeilenschlüsseln; solche Schlüssel bleiben über Store/Import nutzbar.
   - `locora bind PALAST N feld "formel"`, `locora unbind PALAST N feld`, `locora calc [PALAST] "formel"`;
     Nummern wie bei `show`, Felder `place/item/x/y/visible/active`. `calc` ohne Palast nutzt gemeinsame Tabellen,
     mit Palast effektive Tabellen; `View.reveal=false`, Eingabestandards und keine interaktiven Aktionen.
   - `practice/review`: typisierte `Inputs` vor der Routenauswahl (Enter = Standard, ungültige Werte erneut,
     EOF = Abbruch), Groß-/Kleinschreibung bleibt erhalten. Pro Palast eigener flüchtiger Lauf.
     `active` wird einmal mit `View.reveal=true` ausgewertet; Fehler bleiben mit Hinweis auf der Route.
     Inaktive Loci verbrauchen kein Neulocus-Limit. Ort mit `reveal=false`, Inhalt erst beim Aufdecken mit `reveal=true`;
     Bindungsfehler werden angezeigt, ohne die Journey abzubrechen.
   - Recall: `?` listet Aktionen; `!name` führt `set/reveal/reset` vor und nach dem Aufdecken aus, ohne zu bewerten oder
     weiterzugehen. Aktionen sind namensgetreu; nach Änderungen wird das aktuelle Feld neu angezeigt.
     `reset` löscht auch Eingabe-Overrides; Route bleibt der Start-Snapshot. Tabellen/Bindungen werden nie verändert.
     `x/y/visible` sind speicherbare Bindungen, haben aber im reinen Textterminal keine räumliche Darstellung;
     `visible=false` entfernt keinen Locus aus der Route.
   - Sechs Integrationstests in `apps/cli/tests/workbook.rs`: Bereiche/Overrides/Spalten/Typen, Calc/Bind/Unbind,
     Eingaben/Standards/EOF, Aktivität/Aufdecken, Aktionen/Fehler/Atomarität/Reset, Laufisolation und Review-Planung.
     Geprüft mit `cargo fmt --all`, `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`
     und zusätzlich Clippy im Plugin-Workspace. Hooks und Palastdienste siehe Punkt 2.
2. **Hooks im Host und Host-Services für Paläste – erledigt:**
   - API/WIT **0.3** (neue Imports `register-filter`, `call-host`, Export `on-filter`); bestehende 0.2-Komponenten
     müssen mit dem SDK neu gebaut werden. Keine stillschweigende ABI-Kompatibilität.
   - `locora_api::Hook`, `core::JourneyScore`/`Score { palace_id, locus_id, points: u64 }`, `ctx.filter::<H>(priorität, …)`
     und `PluginHost::filter::<H>(wert)`. Reihenfolge aufsteigend nach `(Priorität, Plugin-ID)`, unabhängig von der
     Ladereihenfolge. Genau eine Registrierung pro Hook/Plugin; Duplikate ändern weder Handler noch Priorität.
     Keine Handler = unveränderter Wert; Hot-Reload/Entladen entfernt alte Registrierungen.
   - Jeder Rückgabewert wird typgeprüft. Fehler und ungültige Antworten erzeugen `HandlerError`, erhalten den letzten
     gültigen Wert und lassen das Plugin aktiv; Trap/Panic/Fuel deaktiviert nur das betroffene Plugin. Die Kette läuft
     weiter. Ereignisse werden erst nach der gesamten Kette zugestellt; keine synchrone Wiedereintrittsmöglichkeit.
   - Reservierter nativer Dienst **`core.palaces@1`** über `ctx.palaces(&Request)` bzw. `ctx.call_host::<Palaces>`:
     `List/Get` benötigen `palaces:read`, `Create/Rename/Delete/AddRoom/AddLocus/UpdateLocus/DeleteLocus` benötigen
     `palaces:write`. Prüfung im WIT-Import, auch bei Umgehung des SDK. Schreiben impliziert **kein** Lesen;
     Updates liefern `Done`, keine unveränderten gespeicherten Inhalte. Unbekannte Dienste, fehlerhafte Anfragen,
     fehlender Speicher und fehlende Rechte liefern Fehler. Plugins können den nativen Dienst nicht überschreiben.
   - Store-Adapter nutzt vorhandene Validierung, IDs, Routenreihenfolge und Op-Log; Schnappschüsse ohne Workbook-
     Bindungen oder Medienbytes. Medien nur über bereits vorhandene Hashes; kein Pfad-/Dateizugriff. Koordinaten endlich
     in `[0,1]`, Videozeiten endlich und nicht negativ; Patch-Zeit `None` = erhalten, `Some(None)` = löschen.
   - Erfolgreiches Anlegen/Löschen erzeugt die vorhandenen Palast-/Locus-Ereignisse mit Host-Ursprung; auch der Aufrufer
     empfängt sie nach Rückkehr aus dem Gast. Löschungen bereinigen `review`. Bereits gespeicherte Änderungen benachrichtigen
     andere Plugins auch nach fehlgeschlagenem Start oder beim Beenden. Plugin-ID `host` ist reserviert.
   - CLI-App bindet den gewählten persistenten Profilspeicher **vor** Plugin-Instanziierung an. `plugin dev/run/check`
     verwendet dagegen getrennten flüchtigen Palast- und Plugin-Speicher, niemals das Nutzerprofil. `new/with_kv` ohne
     Palastspeicher bleiben möglich; native Aufrufer über `host.call` sind vertrauenswürdig und benötigen keine Gastrechte.
   - Acht zusätzliche echte WASM-Integrationstests in `crates/locora-host/tests/support/host_services.rs`: Reihenfolge,
     Duplikate/Neuladen/Entladen, Fehler/ungültige Antworten/Panic/Fuel, verzögerte Ereignisse, alle Rechteablehnungen auch
     direkt über WIT, CRUD/Neustart/Op-Log, Medien/Zeitstempel/Validierung, Schreibschutz gegen Inhaltsabfluss sowie
     Löschereignisse bei normalem Betrieb, fehlgeschlagener Aktivierung und Deaktivierung. Dazu CLI-Test für getrennte
     Entwicklungsspeicher. SDK-Anleitung und Grenzen: `crates/locora-sdk/README.md`.
   - Geprüft: `cargo test --workspace`, Clippy mit `--workspace --all-targets -- -D warnings` in beiden Workspaces,
     Formatprüfung in beiden Workspaces. **Grenzen:** kein Rollback bereits gespeicherter Gaständerungen bei Fehlern,
     keine neuen Größen-/Speicherquoten, native Datenbankarbeit nicht durch WASM-Fuel begrenzt. Rechte betreffen den
     Dienstzugriff, nicht den öffentlichen Ereignisbus; Installationszustimmung bleibt Punkt 4. Keine Workbook-Dienste,
     Medienimporte, Raum-Löschung/-Umsortierung oder Sammeltransaktionen in diesem Schritt.
   - Der hier vorbereitete Hook wird seit Punkt 3 von Practice/Review aufgerufen; die Regeln liegen in Core.
3. **`core` als eigenes Plugin – erledigt:**
   - `plugins/core` stellt `core.journey.evaluate@1` bereit. Typisierte API/SDK: `Answer/AnswerKind/QuizOption`,
     `Submission`, `EvaluateRequest/Evaluation`, `Scored`; API/WIT **0.4**. Antwortfelder ändern die Postcard-DTOs
     des Palastdienstes: 0.3-Komponenten neu bauen, nicht umetikettieren. Vorhandene Hooks/Rechte bleiben erhalten.
   - Recall bleibt die bisherige Selbsteinschätzung (Again/Hard/Good/Easy; außer Again jeweils richtig).
     Guess vergleicht die zum Abgabezeitpunkt mit `View.reveal=true` aufgelöste Item-Bindung mit der Eingabe,
     außen getrimmt und Unicode-kleingeschrieben. Leere Sollantworten/Formelfehler bleiben unbewertet mit Diagnose.
     Quiz hat 2–9 explizite, nichtleere, normalisiert eindeutige Optionen, davon mindestens eine richtig und eine
     falsch; mehrere richtige möglich. Nur die vollständige richtige Menge zählt, keine Teilpunkte, kein Shuffle.
   - Falsch = Again, automatisch richtig bei **≤ 5.000 ms = Easy**, sonst Good. Zeit ab erster Anzeige bis Abgabe,
     einschließlich Aktionen und ungültiger Wiederholungen; Recall inklusive Selbsteinschätzung. Explizite erste
     Locora-Regel, keine behauptete Übernahme historischer mnemodim-Timer/XP. Basis: richtig 1 Punkt, falsch 0.
   - Der native Client besitzt weiterhin Routensnapshot, Workbook-Lauf, Terminal und Uhr. Er ruft gezielt den
     Dienstanbieter `core` auf (keine Übernahme durch konkurrierende Anbieter). Profil-Plugins dürfen bereits
     geladene IDs nicht ersetzen; explizites Entwicklungs-Hot-Reload bleibt möglich. Er ruft Core auf und
     filtert danach genau einmal über `core.journey.score`; nur `points` werden übernommen, IDs bleiben vertrauenswürdig.
     Summen sättigen bei `u64::MAX`. `LocusAnswered` mit Core-Grade plant unverändert Review/FSRS, `Scored` meldet
     Endpunkte. Filter ändern weder Richtigkeit noch Grade. `JourneyCompleted` nur für nichtleere, vollständig
     beantwortete ausgewählte Palastrouten; bei Abbruch bleiben bereits beantwortete Loci geplant.
   - `locora edit PALAST N --answer '{"type":"guess"}'` bzw. Quiz-JSON; Recall per `{"type":"recall"}`.
     Neue Loci starten als Recall; Quizoptionen erscheinen in der Abfrage ohne Korrektheitsmarkierung.
     Quiz-Eingabe: einbasierte Nummern,
     kommagetrennt; Duplikate/ungültige Nummern wiederholen. Guess: `=q`, `=?`, `=!name`, `==text` für wörtliche
     Antworten statt Steuerbefehlen. Enter oder Workbook-Reveal bei automatischen Fragen = Forfeit/Again.
     Bei vorhandenen Workbook-Aktionen bleibt das Feedback am aufgedeckten Locus: `?`/`!name` ohne erneute
     Bewertung, Enter weiter, q/EOF beendet (bereits abgegebene Antwort bleibt geplant). Ohne Aktionen direkt
     weiter nach Feedback. Recall-Aktionen unverändert.
   - SQLite-Migration 4 ergänzt Antwort-JSON mit Recall-Standard, ohne Route/Bindings/Review-KV zu verändern.
     Store und Palastdienst prüfen vor Schreiben/Op-Log; Patch `None` erhält, explizites Recall setzt zurück.
     `.locora` v1 bleibt für reine Recall-Exporte unverändert (kein `answer`); Quiz/Guess erzeugen v2. Leser
     akzeptieren v1/v2, weisen nicht-Recall in v1 zurück; alte Leser lehnen v2 statt stiller Uminterpretation ab.
   - Tests: Migration/Atomarität, v1/v2-Roundtrips und Ablehnungen, Antwort-DTOs, echter Core-WASM-Dienst,
     Quiz-Mengen/Timergrenze/Forfeit/Neuladen, Filter-Fehler und FSRS-Unabhängigkeit, gemischte CLI-Practice/Review,
     Bindungen, Abbruch/EOF, wörtliche Steuerzeichen, leere/defekte Sollantworten und Import/Export;
     konkurrierende Bewertungsanbieter, Schutz geladener Plugin-IDs und Aktionen nach automatischem Feedback.
   - **Grenzen:** kein XP-/Combo-/Trophäensystem oder persistentes Punktekonto, keine automatische Distraktorerzeugung,
     kein Legacy-Kommaantwort-Parser, kein zusätzlicher Antwort-Editor, keine unscharfe Guess-Prüfung. Erwartete
     Antworten, Zeitmessung und Forfeit kommen vom vertrauenswürdigen nativen Client (kein Replay-/Anti-Cheat-Protokoll).
     Ohne Core (z.B. `LOCORA_SKIP_PLUGINS=1`) funktionieren Bearbeitung/Transfer, aber Practice/Review melden vor
     Eingaben einen Fehler; kein nativer Ersatz-Evaluator. Installation, Benachrichtigungen und App-UI nicht erweitert.
   - Geprüft: `cargo test --workspace` **76 Tests + 2 Doctests bestanden, 2 Doctests ignoriert**;
     Plugin-Workspace nativ **2 Tests bestanden**. Clippy `--workspace --all-targets -- -D warnings` und
     `cargo fmt --all -- --check` in beiden Workspaces erfolgreich; `git diff --check` sauber.
     Release-Build und manuelle Prüfung eines echten Schema-3-Profils erfolgreich: bestehende Tabelleninhalte,
     Bindungen und Review-KV unverändert; alte v1-Archive importierbar, reine Recall-Exporte weiterhin v1.
     Keine Commits/Pushes; vorhandene uncommittete Änderungen aus Punkt 2 bleiben enthalten.
   - Anleitung/Verträge: `crates/locora-sdk/README.md`, `format/README.md`. **Nächster offener Schritt nach Punkt 4: Punkt 5.**
4. **`locora plugin install/remove/ls` – erledigt:**
   - Lokale WASM-Komponenten, derselbe Profilpfad wie sonst (`--data`, `LOCORA_DATA`, Plattformstandard).
     `install FILE [--yes]` zeigt kurze Desktop-Warnung, Identität/API und Rechte; interaktiv `j/ja`, Standard Abbruch.
     Nichtinteraktiv ausschließlich mit ausdrücklichem `--yes`; gepiptes `ja` genügt nicht. Keine Profiländerung bei Ablehnung.
   - `PluginHost::inspect_bytes` prüft Manifest/API ohne Aktivierung, Profilzugriff oder Cache-Schreiben. Start-/Manifestcode
     läuft mit Fuel, ohne Rechte und mit isolierten Backends; Gastmeldungen werden verworfen. Auch `check` aktiviert nicht mehr.
     Zulässig: `storage`, `palaces:read`, `palaces:write`; `notify/network` und unbekannte Rechte werden abgelehnt.
   - Ablage `<data>/plugins/<id>.wasm` plus bestätigtes Manifest `<id>.wasm.json`; nur sichere IDs (1–128 Zeichen,
     a-z/0-9/-/_, Beginn a-z/0-9). Exakt geprüfte Bytes, atomare Veröffentlichung ohne Überschreiben, Zustimmung zuerst.
     Vorhandene Ziele/Zustimmungen und doppelte IDs auch unter anderem Dateinamen abgelehnt; kein Update-Befehl.
   - Installation aktiviert auch nach Zustimmung **nicht**. Erst normale Profilbefehle laden mit `load_approved_file`:
     gleiche Duplikatsperre wie `load_additional_file`, zusätzlich exakter Manifestvergleich vor Rechtevergabe/Aktivierung.
     Damit können dynamische Manifeste keine ungeprüften Rechte nachfordern. Fehlende/defekte Zustimmung sperrt Aktivierung;
     manuell kopierte Dateien müssen über `install` gehen. `core/review` auch bei fehlenden eingebetteten Bytes geschützt.
     Native Bewertung bleibt gezielt `host.call_plugin("core")`; keine Konkurrenzübernahme.
   - `ls` liest Metadaten ohne Aktivierung, meldet defekte/inkompatible Dateien und fehlende/unpassende Zustimmung.
     `remove ID` löst auch umbenannte Dateien über isolierte Metadaten auf und lehnt mehrdeutige IDs ab;
     `remove DATEINAME.wasm` benötigt keinerlei Gastcode. Beide entfernen Profilkopie samt Zustimmung ohne Aktivierung,
     erhält Plugin-KV und Paläste. Defekte Profilkopien auch unter `core.wasm/review.wasm` entfernbar, eingebettete Plugins nie.
     Verwaiste Zustimmung nach Abbruch ebenfalls über `remove ID` bereinigbar; Neuinstallation nach explizitem Entfernen.
   - Tests: zehn CLI-Integrationstests mit echten WASMs (gemeinsames Workspace-Fixture-Baumuster), neue `install-probe`
     mit verbotenen Manifestzugriffen und stets abstürzender Aktivierung; Ablehnung ohne Seiteneffekte, Rechte/API/IDs,
     Duplikate/Alias/kaputte Ziele, Auflisten/Entfernen defekter Dateien, Zustimmungsmismatch, Profilaktivierung und KV-Erhalt.
     Host-Tests für nebenwirkungsfreie Inspektion, Zustimmungsprüfung, Duplikate und reservierte IDs; Unit-Tests für sichere
     IDs (inkl. Windows-Gerätenamen), alle unterstützten/nicht unterstützten Rechte und atomare No-Clobber-Schreibvorgänge.
     Nichtreguläre Dateien/Symlinks werden vor dem Lesen abgelehnt; Unix-Tests für FIFO-Komponenten/-Zustimmungen,
     Link-Entfernung und verlinkte Plugin-Verzeichnisse. Verzeichnisprüfung vor Profilöffnung; IDs/Dateinamen werden nicht verwechselt.
     API-Syntax vollständig numerisch (major.minor[.patch]); ungültige Suffixe werden ebenfalls abgelehnt.
   - Verifiziert: `cargo test --workspace` (87 Tests + 2 Doctests bestanden; 2 Doctests ignoriert),
     Plugin-Workspace-Tests (2 bestanden), `cargo fmt --check` und `cargo clippy --workspace --all-targets -- -D warnings`
     in beiden Workspaces. TTY-Dialog zusätzlich mit Standardantwort, EOF und ausdrücklichem `ja` geprüft.
   - **Grenzen:** keine Signaturen (Ed25519 später), Pakete, Registry, Downloads oder Update-Verwaltung. Profil/Zustimmung
     sind vertrauenswürdige lokale Nutzerdaten; kein Schutz gegen lokale Manipulation. Fuel begrenzt keine Kompilierung,
     Speicherallokation oder Host-Puffer; keine neuen Ressourcenquoten. `dev/run` bleiben explizite isolierte Entwicklungsausführung.
     Anleitung/Verträge: `README.md`, `crates/locora-sdk/README.md`. Nächster Schritt: Punkt 5.
5. **`locora.toml`-Quelltextformat** + `locora build` (Paläste in Git pflegen, per KI erzeugen).
6. **`.locora` mit Fortschritt:** `locora export --with-progress` schreibt Review-Daten nach `plugins/review/`.
7. **Review-Log** (jede Antwort speichern) und FSRS-Parameteroptimierung; Tageslimits für neue Loci in den Einstellungen.
8. **Sammlungen** (mehrere Paläste mit gemeinsamer Navigation, `navigate`-Aktion aus mnemodim) – erst wenn gebraucht.

