# Locora – Plan für den Rust-Neubau von mnemodim

> **Locora** = *loci* (die Loci-Methode) + *ora* (lat. „Küste/Rand“, klingt wie „Aura“).
> Kurz, aussprechbar auf Deutsch und Englisch, auf crates.io und unter `maro-git/` noch frei.
> CLI-Befehl: `locora` (Alias `loc`). Dateiendung: `.locora`.
> Alternativen, falls der Name nicht gefällt: `mnemora`, `palastra`, `lokari` (alle ebenfalls frei).

---

## 1. Ziele

| Ziel | Was das konkret heißt |
|---|---|
| **Pures Rust** | Logik *und* UI sind Rust: Datenmodell, Formeln, Journeys, Format, Plugins, Sync, CLI und Oberfläche (Leptos → WASM). Kein JavaScript-Framework, kein Node im Build. |
| **Offline first** | Lokale SQLite-Datenbank ist die Wahrheit. Kein Account nötig. Die Cloud (später) synchronisiert nur. |
| **Sehr schnell** | Start in < 300 ms, Journey-Schritte ohne Netz, Medien lokal und nach Inhalt adressiert. |
| **Alles ist ein Plugin** | `core`, `review` (Spaced Repetition), `notify` (Benachrichtigungen), `tutorial` und `gamification` sind Plugins mit derselben API. Der Host selbst kann nichts außer Plugins laden. |
| **Open Core** | Die ganze App inkl. `gamification` ist Open Source (MIT) und kostenlos. Nur der Web-Service **Locora Cloud** (Ranglisten, Sync, gemeinsame Challenges) ist proprietär und kostet ein günstiges Abo. |
| **Wenig Klicks** | Für jede häufige Aufgabe gibt es ein festes Klick-Budget (siehe Abschnitt 6). |
| **CLI** | Paläste in Sekunden aus dem Terminal anlegen, auf derselben Datenbank wie die App. |
| **Video-Paläste** | Ein Video läuft, hält bei einem Locus an und läuft erst weiter, wenn der Locus gelöst ist. |

---

## 2. Framework-Entscheidung

**Entscheidung: Tauri 2 + Leptos – alles in Rust**

| Framework | Desktop | Android | iOS | Video | Reife | Urteil |
|---|---|---|---|---|---|---|
| **Tauri 2** | ✅ | ✅ | ✅ | ✅ natives `<video>` der WebView | stabil, große Community | **Empfehlung** |
| Dioxus 0.7 | ✅ | ✅ | ✅ | ✅ (WebView-Renderer) | Mobile noch jung | gute All-Rust-Alternative |
| Slint | ✅ | ✅ | ⚠️ experimentell | ❌ eigenes Decoding (GStreamer/FFmpeg) nötig | gut für Embedded | nicht passend |
| egui / iced | ✅ | ⚠️ | ⚠️ | ❌ eigenes Decoding nötig | gut für Tools | nicht passend |

Den Ausschlag geben die **Video-Paläste**. Mit einer WebView bekommt man auf allen fünf Plattformen
Hardware-Video-Decoding gratis. Native Rust-GUIs müssten FFmpeg/GStreamer für Android und iOS
mitbringen, und das ist ein Projekt für sich.

**UI-Schicht: Leptos (Rust → WASM)**, gebaut mit `trunk` bzw. `cargo leptos`.

- Feingranulare Signale ohne Virtual DOM: Updates betreffen nur die geänderten DOM-Knoten, daher sehr schnell und kleines WASM-Bundle.
- Gemeinsame Typen: UI, Tauri-Commands, Plugins und CLI nutzen dieselben Rust-Structs (`locora-sdk`). Es gibt keine doppelten TS-Typen und keine Serialisierungsfehler zwischen Sprachen.
- Die UI enthält **keine** Fachlogik. Sie ruft Tauri-Commands auf und hört auf Host-Events.
- Video, Zeichnen und Gesten laufen über `web-sys` direkt auf `<video>`, `<canvas>` und Pointer-Events.
- Release-Profil: `lto = "fat"`, `codegen-units = 1`, `opt-level = 3` (Core) bzw. `"z"` + `wasm-opt` (UI).

---

**Grafiken: Splinely.** Alle Grafiken (Maskottchen, Icons, Illustrationen, Animationen) werden mit `splinely-cli`
als `.sply`-Dateien erstellt und in der App über die Splinely-Runtime abgespielt. Sie liegen unter `assets/`.

**Maskottchen: Simo**, benannt nach Simonides von Keos, dem Erfinder der Loci-Methode: ein Mann mit hellbraunem Hautton und
Afro als Ganzkörperfigur, ohne Konturen und mit Cel-Shading. An der Jacke leuchten drei Loci nacheinander auf (die Loci-Methode in
einem Bild), in der Hand trägt er eine Laterne voller Glühwürmchen-Licht, und ein Glühwürmchen begleitet ihn. Simo begleitet
Tutorial, leere Zustände, Erfolge (Gamification) und Benachrichtigungen. Stil-Leitfaden: [`assets/STIL.md`](assets/STIL.md). App-Icon: Palast-Torbogen mit leuchtenden Loci und dem Licht des Glühwürmchens.

---

## 3. Repository-Struktur

```text
locora/                          (öffentlich, MIT)
├── crates/
│   ├── locora-sdk/              Plugin-API: Traits, Events, Services, UI-Beiträge, WIT-Definition
│   ├── locora-host/             wasmtime-Plugin-Loader, Hot-Reload, Event-Bus, Service-Registry, Berechtigungen
│   ├── locora-store/            SQLite (rusqlite), Migrationen, Operations-Log, Medien-Store
│   ├── locora-format/           .locora lesen/schreiben
│   ├── locora-formula/          Port von shared/formulas.ts + workbookRuntime.ts
│   └── locora-sync/             (später) Op-Log-Sync-Client
├── plugins/
│   ├── core/                    Paläste, Räume, Loci, Video-Räume, Journeys, Workbook
│   ├── review/                  Spaced Repetition (FSRS): fällige Loci, Planung
│   ├── notify/                  Benachrichtigungen: System, In-App, Zeitplanung, Ruhezeiten
│   ├── tutorial/                Onboarding, interaktive Touren, Formel-Guide
│   └── gamification/            XP, Herzen, Combos, Trophäen, Daily Challenges (lokal, kostenlos) + Cloud-Client
├── apps/
│   ├── cli/                     `locora` Binary (clap)
│   └── app/                     Tauri 2 (src-tauri/) + Leptos-UI (ui/)
├── format/                      Spezifikation .locora + Beispiel-Dateien + JSON-Schema
└── docs/

locora-cloud/                    (privat, proprietär)
└── server/                      axum + Postgres: Abo, Ranglisten, gemeinsame Challenges, Sync
```

Die App ist vollständig Open Source. Das `gamification`-Plugin enthält einen Client für Locora Cloud,
der nur mit einem aktiven Abo Daten an den Server schickt. Ohne Abo läuft alles lokal.

---

## 4. Plugin-Architektur

### 4.1 Grundprinzip

Der Host kann nur drei Dinge: **Plugins laden**, **Nachrichten vermitteln** und **UI-Beiträge anzeigen**.
Alles Fachliche steckt in Plugins, auch der Kern.

**Plugins werden zur Laufzeit geladen** (siehe 4.4). Man schreibt sie in Rust und kompiliert sie zu einer
`.wasm`-Komponente. Die App lädt, entlädt und aktualisiert sie, ohne neu zu starten. Plugins schreibt man so:

```rust
use locora_sdk::prelude::*;

#[locora_plugin]                       // erzeugt Manifest-Export und WIT-Bindings
struct Review;

impl Plugin for Review {
    fn activate(ctx: &mut Ctx) -> Result<()> {
        ctx.on::<LocusAnswered>(|ctx, ev| schedule(ctx, ev))?;
        ctx.provide::<DueService>(DueImpl)?;
        Ok(())
    }
}
```

`PluginContext` bietet:

| Baustein | Zweck | Beispiel |
|---|---|---|
| **Events** (Pub/Sub) | Etwas *ist passiert*, ohne Antwort | `core.journey.locus_answered`, `core.palace.created` |
| **Services** (Request/Response) | Ein Plugin bietet eine versionierte Schnittstelle an | `core.palaces@1`, `tutorial.highlight@1` |
| **Hooks** (Filter-Ketten) | Plugins dürfen ein Ergebnis verändern | `core.journey.score` → Gamification addiert Combo-Bonus |
| **Storage** | Eigener Namensraum in SQLite (Tabellen + KV) | `gamification.xp` |
| **UI-Beiträge** | Deklarativ: Kachel, Befehle, Panels, Locus-Typen, Overlays | Gamification fügt ein XP-Overlay in die Journey ein |
| **CLI-Beiträge** | Eigene Unterbefehle | `locora tutorial reset`, `locora stats` |

Payloads sind serde-Structs. Sie werden im SDK als Typen veröffentlicht und intern als JSON/Postcard übertragen.
Dieselben Typen gelten auf beiden Seiten der WASM-Grenze.

### 4.2 So reden die Plugins miteinander

```text
core ──event: journey.locus_answered──▶ gamification (Combo, Herz-Verlust)
core ──hook:  journey.score ──────────▶ gamification (Multiplikator) ──▶ zurück an core
core ──event: palace.created ─────────▶ tutorial (nächster Tour-Schritt)
core ──event: journey.locus_answered──▶ review (FSRS-Bewertung, neuer Fälligkeitstermin)
core ──hook:  journey.roster ─────────▶ review (nur fällige Loci abfragen)
core/host ──service: review.due@1 ────▶ review (Badges auf Palastkarten, Weiter-Zeile)
review ──service: notify.schedule@1 ──▶ notify (Erinnerung „12 Loci fällig“)
gamification ──service: notify.send@1 ▶ notify (Streak in Gefahr, Trophäe freigeschaltet)
review ──event: review.session_done ──▶ gamification (Streak, Daily Challenge „alles Fällige erledigt“)
tutorial ──service: core.palaces@1 ───▶ core (Beispielpalast anlegen)
tutorial ──service: ui.highlight@1 ───▶ host (Element hervorheben)
gamification ──event: trophy.unlocked ▶ tutorial (erklärt Trophäen beim ersten Mal)
```

Alle Abhängigkeiten außer `core` sind **optional**. Fehlt Gamification, rechnet `core` die Journey
einfach ohne Hook. Fehlt `review`, gibt es keine Fälligkeiten und die Weiter-Zeile zeigt den zuletzt bearbeiteten Palast. Fehlt Tutorial, passiert gar nichts.

### 4.3 Open Core: was ist offen, was kostet

| Teil | Lizenz | Preis |
|---|---|---|
| Host, SDK, Store, Format, Formula, CLI | MIT | frei |
| Plugin `core` | MIT | frei, voller Funktionsumfang offline |
| Plugin `tutorial` | MIT | frei |
| Plugin `gamification` | MIT | frei, XP/Trophäen/Challenges lokal |
| **Locora Cloud** (Server) | proprietär | Abo: Ranglisten, Freunde, gemeinsame Challenges, Sync über Geräte |

Weil alles unter MIT steht, dürfen auch Drittanbieter eigene (auch kommerzielle) Plugins bauen.
Der Wert des Abos liegt im Dienst (Server, Community, Ranglisten), nicht im Code. Für externe Beiträge in den
offenen Repos sollte man ein DCO/CLA einführen, damit du die Lizenz später noch ändern kannst.

**Freischaltung offline:** Der Server stellt ein Ed25519-signiertes Entitlement-Token aus
(`service`, `plan`, `expires`). Die App prüft es lokal mit dem eingebauten Public Key und gewährt
14 Tage Kulanz ohne Netz. Es gibt kein „Phone home“ bei jedem Start.

### 4.4 Plugins zur Laufzeit laden: Rust → WebAssembly-Komponenten

**Entscheidung:** Plugins werden in Rust geschrieben und nach `wasm32-wasip2` als **WebAssembly-Komponente** kompiliert.
Der Host lädt sie mit **wasmtime** zur Laufzeit. Die Plugin-API ist ein **WIT-Interface** (`crates/locora-sdk/wit/`).
Das `locora-sdk` versteckt WIT komplett, Plugin-Autoren schreiben normales Rust.

| Ansatz | Laufzeit laden | Hot-Reload | Sicher (Sandbox) | Android | iOS | Urteil |
|---|---|---|---|---|---|---|
| **WASM-Komponenten (wasmtime)** | ✅ | ✅ | ✅ | ✅ (Cranelift) | ✅ (Interpreter Pulley) | **gewählt** |
| Native `.so`/`.dll`/`.dylib` (`libloading`) | ✅ | ⚠️ | ❌ volle Rechte | ⚠️ Play-Store verbietet nachgeladenen Code | ❌ verboten | abgelehnt |
| Statisch einkompiliert | ❌ | ❌ | – | ✅ | ✅ | nur als Fallback |

Native Libraries scheiden aus: Rust hat keine stabile ABI, ein Plugin hätte vollen Zugriff auf das Gerät,
und iOS bzw. der Play Store verbieten nachgeladenen nativen Code.

**Geschwindigkeit**

- Desktop und Android kompilieren WASM per **Cranelift** zu nativem Maschinencode, typisch 1,1–1,5× nativer Laufzeit.
  Das Kompilat wird gecacht (`~/.cache/locora/plugins/<hash>.cwasm`), der zweite Start lädt in Millisekunden.
- iOS verbietet JIT. Dort läuft wasmtimes Interpreter **Pulley**. Das reicht für Plugin-Logik (Events, Planung, Punkte),
  weil die schweren Teile (SQLite, Formeln, Medien, Video) im nativen Host liegen und Plugins sie über Services aufrufen.
- Aufrufe über die Grenze werden gebündelt (Events in Batches, Daten als kompaktes Postcard statt JSON).
- Hot Paths, die messbar zu langsam sind, wandern als Service in den Host. Das Plugin bleibt die Logik-Schicht.

**Laufzeit-Verhalten**

- **Installieren:** `.locora-plugin`-Paket (ZIP: `plugin.wasm` + `manifest.toml` + Icon + Signatur) per Drag-and-drop, CLI oder später Plugin-Verzeichnis.
- **Laden/Entladen/Update ohne Neustart:** Der Host ruft `deactivate`, tauscht die Komponente und ruft `activate`.
  Plugin-Zustand liegt in SQLite, nicht im WASM-Speicher, und überlebt das Neuladen.
- **Hot-Reload für Entwickler:** `locora plugin dev ./plugins/review` beobachtet den Ordner, kompiliert bei jeder Änderung neu
  und lädt das Plugin live in die laufende App.
- **Absturzsicher:** Ein Panic oder eine Endlosschleife (Fuel/Epoch-Limit) beendet nur dieses Plugin. Die App läuft weiter und zeigt einen Hinweis.
- **Berechtigungen** werden im Manifest deklariert (`storage`, `palaces:read`, `palaces:write`, `notify`, `network`) und beim Installieren bestätigt.
  Ohne Berechtigung existiert die Host-Funktion für das Plugin nicht (Capability-basiert).
- **Signaturen:** Offizielle Plugins sind mit Ed25519 signiert. Unsignierte Plugins gehen nur im Entwicklermodus.

**UI aus Plugins**

Ein zur Laufzeit geladenes Plugin kann keine Leptos-Komponenten in die fertig kompilierte App einbauen. Deshalb:

- **Deklarative UI (Standard):** Das Plugin liefert einen View-Baum aus festen Bausteinen (Kachel, Liste, Button, Formular,
  Overlay, Badge, Diagramm, Canvas-Zeichenbefehle). Die App rendert ihn mit nativen Leptos-Komponenten. Das ist schnell,
  sieht einheitlich aus und hält die Klick-Regeln aus Abschnitt 6 ein.
- **Eigene UI (Ausnahme):** Ein Plugin kann zusätzlich ein eigenes Leptos-WASM-Bundle mitliefern. Das läuft in einem sandboxed iframe
  mit Nachrichten-Brücke zum Plugin.

**Fremde Plugins auf dem Handy: Warnung, dann auf eigene Verantwortung**

Die offiziellen Plugins (`core`, `review`, `notify`, `tutorial`, `gamification`) sind signiert und kommen mit der App.
Für alle anderen Plugins gilt auf **Android und iOS** (auf Desktop in Kurzform, siehe unten):

```text
┌──────────────────────────────────────────────────┐
│ ⚠  Plugin aus unbekannter Quelle                 │
│                                                  │
│ „Vokabel-Import“ von maxmuster ist nicht von     │
│ Locora geprüft. Das Installieren von Plugins ist │
│ gefährlich und geschieht auf eigene Verantwortung.│
│                                                  │
│ Es möchte:  ✓ Paläste lesen  ✓ Paläste ändern     │
│                                                  │
│ [ Abbrechen ]        [ Trotzdem installieren ]   │
└──────────────────────────────────────────────────┘
```

- Der Dialog erscheint bei jeder Installation und bei jedem Update, das neue Rechte verlangt. Abbrechen ist die Standard-Taste.
- **Desktop** (Windows, macOS, Linux): Auch hier kommt vor jeder Installation eine kurze Warnung:
  „⚠ Nicht von Locora geprüft – Installation auf eigene Verantwortung. Rechte: Paläste lesen. [Abbrechen] [Installieren]“.
  Im Entwicklermodus (`locora plugin dev`) erscheint sie einmal pro Plugin und nicht bei jedem Hot-Reload.
- Ein Plugin, das abstürzt oder zu lange rechnet, wird automatisch deaktiviert.
- **Risiko App Store:** Die Warnung schützt den Nutzer, aber nicht vor einer Ablehnung durch Apple oder Google
  (Apple 2.5.2 und Google Play „Device and Network Abuse“). Vor dem ersten Store-Release prüfen.
  **Ausweichlösung (abgestimmt):** Falls das Laden fremder Plugins auf Android oder iOS technisch oder wegen der Store-Regeln schwierig wird,
  schalten wir es dort einfach ab (Build-Flag). Die mitgelieferten offiziellen Plugins laufen weiter.

### 4.4.1 Eigene Plugins bauen: so einfach wie möglich

Jeder Nutzer soll ein Plugin in **wenigen Minuten** bauen können, ohne WIT, wasmtime oder Build-Details zu kennen.

**In drei Befehlen vom Nichts zum laufenden Plugin**

```bash
locora plugin new mein-plugin            # fragt nach Vorlage, legt fertiges Projekt an
cd mein-plugin
locora plugin dev                        # baut, installiert und lädt live in die offene App neu
```

- `locora plugin dev` installiert fehlende Teile selbst (z. B. `rustup target add wasm32-wasip2`), mit Rückfrage.
- Jede Änderung wird in unter einer Sekunde neu gebaut und live geladen. Zustand bleibt erhalten.
- `locora plugin dev --device` schickt das Plugin über WLAN an Handy oder Tablet (Pairing per QR-Code). So testet man mobil ohne App-Store.
- `locora plugin pack` erzeugt die `.locora-plugin`-Datei zum Weitergeben. `locora plugin check` prüft Manifest, Rechte und API-Version.

**Vorlagen** (`locora plugin new --template …`)

| Vorlage | Ergebnis |
|---|---|
| `kachel` | eigene Kachel auf dem Startbildschirm |
| `befehl` | Befehl in der Befehlspalette und in der CLI |
| `locus-typ` | neue Abfrageart (z. B. „Bild zeichnen“, „Reihenfolge sortieren“) |
| `importer` | Paläste aus anderen Quellen (CSV, Anki, Text) erzeugen |
| `ereignis` | auf Events reagieren (z. B. nach jeder Journey etwas tun) |

**Ein komplettes Plugin passt in eine Datei:**

```rust
use locora_sdk::prelude::*;

#[locora_plugin(name = "Lernzeit", permissions = [palaces_read])]
struct Lernzeit;

impl Plugin for Lernzeit {
    fn activate(ctx: &mut Ctx) -> Result<()> {
        ctx.on::<JourneyCompleted>(|ctx, ev| {
            let minuten = ctx.store().add("minuten", ev.duration.as_secs() / 60)?;
            ctx.tile("Lernzeit", format!("{minuten} min"));
            Ok(())
        })
    }
}
```

**Hilfen für Plugin-Autoren**

- Das SDK nutzt normale Rust-Typen mit Autovervollständigung und Doku in der IDE. Fehler kommen als verständliche Meldungen:
  „Plugin braucht die Berechtigung `palaces_write`, füge sie in `#[locora_plugin]` hinzu“.
- **Entwicklerpanel in der App** (nur im Entwicklermodus): Live-Log, empfangene Events, Service-Aufrufe, Speicher-Inhalt, Neu-laden-Knopf.
- **Testen ohne App:** `locora_sdk::testing` simuliert Host, Events und Speicher. `cargo test` reicht.
- **Dokumentation:** Schritt-für-Schritt-Tutorial, Rezepte für alle Vorlagen, API-Referenz (`docs.rs`) und eine `llms.txt`,
  damit auch KI-Assistenten korrekte Plugins schreiben.
- **API-Stabilität:** WIT-Interface mit Versionsnummer. Plugins für API 1.x laufen in allen 1.x-Versionen der App.

### 4.5 Plugin `review`: Spaced Repetition

Plant, **wann** welcher Locus wiederholt werden muss, damit man ihn nicht vergisst.

- **Algorithmus:** FSRS über die Crate `fsrs` (pures Rust, vom Open-Spaced-Repetition-Projekt).
  Jeder Locus ist eine Karte mit Stabilität, Schwierigkeit und Fälligkeitsdatum. Ziel-Behaltensrate einstellbar, Standard 90 %.
- **Persönliche Optimierung:** Aus dem lokalen Wiederholungs-Log werden die FSRS-Parameter auf dem Gerät neu berechnet
  (automatisch alle ~1.000 Wiederholungen oder per `locora review optimize`). Es gibt keinen Server dafür.
- **Bewertung ohne Extra-Klicks:**

  | Locus-Typ | Bewertung |
  |---|---|
  | Recall | Tasten **1–4** (Nochmal / Schwer / Gut / Leicht) direkt nach dem Aufdecken, auf Mobile Wischgesten |
  | Quiz / Guess | automatisch: falsch → *Nochmal*; richtig → *Gut* oder *Leicht* je nach Antwortzeit |

- **Räumliche Reihenfolge bleibt erhalten:** Die Loci-Methode lebt von der Route. Im Fällig-Modus läuft die Journey trotzdem
  den Palast in Routen-Reihenfolge ab. Nicht fällige Loci werden kurz gezeigt (Orientierung), aber nicht abgefragt.
  Video-Paläste springen jeweils ein paar Sekunden vor den nächsten fälligen Locus.
- **Ganze Paläste:** Ein Palast gilt als fällig, sobald einer seiner Loci fällig ist. Die Palastkarte zeigt ein Badge („7“),
  ▶ startet direkt den Fällig-Modus.
- **Tageslimits:** maximale neue Loci pro Tag und maximale Wiederholungen. Neue Loci werden zuerst „eingeführt“ (einmal ansehen, dann abfragen).
- **Erinnerungen:** über den Service `notify.schedule@1` des Plugins `notify` (siehe 4.6), z. B. täglich „12 Loci fällig“. Ohne `notify` gibt es nur das Badge in der App.
- **Speicher:** eigene Tabellen `review.cards` und `review.log` (Op-Log, also später mit Locora Cloud synchronisierbar).
  Im Export optional über `--with-progress` unter `plugins/review/` enthalten.
- **CLI:** `locora due`, `locora review [Palast]`, `locora review stats`, `locora review optimize`.

### 4.6 Plugin `notify`: Benachrichtigungen

Eine zentrale Stelle für **alle** Benachrichtigungen. Kein anderes Plugin spricht direkt mit dem Betriebssystem.
So gibt es eine Einstellungsseite, einheitliche Ruhezeiten und keine Benachrichtigungsflut.

- **Services für andere Plugins:**
  - `notify.send@1`: sofort anzeigen (In-App-Toast und/oder System-Benachrichtigung).
  - `notify.schedule@1`: zu einem Zeitpunkt oder wiederkehrend planen (`daily 18:00`), mit Schlüssel zum Ersetzen/Abbrechen.
  - Jede Nachricht hat Plugin, Kanal (z. B. `review.due`, `gamification.streak`), Priorität und eine **Aktion**
    (z. B. `review.start@Planeten`). Ein Tipp auf die Benachrichtigung öffnet direkt die passende Ansicht: **0 Zusatzklicks**.
- **Ausgabe:**

  | Plattform | Umsetzung |
  |---|---|
  | Desktop (Windows/macOS/Linux) | System-Benachrichtigungen (`tauri-plugin-notification`); Zeitplanung durch den Host, solange die App oder der Tray-Prozess läuft |
  | Android / iOS | vom System geplante lokale Benachrichtigungen, funktionieren auch bei geschlossener App |
  | In der App | Toasts und eine Glocke mit Verlauf |
  | CLI | `locora notify ls`; optional Ausgabe beim Start von `locora` („12 Loci fällig“) |

- **Offline:** Alles ist lokal geplant und in SQLite gespeichert (`notify.queue`, `notify.settings`). Push über Locora Cloud (z. B. „Freund hat dich überholt“) kommt erst später als optionaler Kanal.
- **Gegen Nerven:** Ruhezeiten, Tageslimit pro Kanal, und Bündeln: Nachrichten im selben Zeitfenster werden zu **einer** zusammengefasst
  („12 Loci fällig · Streak endet heute“). Jeder Kanal lässt sich einzeln abschalten.
- **Ohne `notify`:** Plugins laufen weiter, Hinweise erscheinen nur als Badges in der App.
- **CLI:** `locora notify ls`, `locora notify mute <kanal>`, `locora notify quiet 22:00-08:00`.

---

## 5. Datenmodell und Offline first

- **SQLite** pro Profil (`~/.local/share/locora/` bzw. App-Sandbox auf Mobile), WAL-Modus.
- **Operations-Log:** Jede Änderung wird als Op mit Hybrid-Logical-Clock-Zeitstempel gespeichert
  (Last-Writer-Wins pro Feld, Tombstones für Löschungen). Das reicht für den späteren Sync,
  ohne dass jetzt schon ein Server existieren muss.
- **Medien:** Sie werden nach SHA-256 benannt (`media/ab/cd…`). Das liefert automatisches Deduplizieren, einfachen Sync und unveränderliche Dateien.
- **Journeys** werden vollständig lokal bewertet. Eine Server-Validierung (wie bisher in Convex) gibt es nur
  für Gamification-Ranglisten. Dort wird das Transkript wie heute serverseitig nachgespielt, mit derselben Rust-Crate kompiliert für den Server.
- **Wiederholung:** übernimmt das eigene Plugin `review` (siehe 4.5). Das bisherige *Mastery* wird auf dessen Stabilitätswerte abgebildet.

### Was aus mnemodim wohin wandert

| mnemodim heute | Locora |
|---|---|
| Paläste, Stages (jetzt **Räume**), Loci, Recall/Quiz/Guess | `core` |
| Workbook, Formeln, Major-Tabellen, Actions, Collections | `core` (`locora-formula`) |
| Import/Export `.mnemodim` | neues Format `.locora` (`locora-format`); `.mnemodim` wird nicht mehr unterstützt |
| Timing-Punkte, Combos, Herzen, XP, Trophäen, Daily Challenges | `gamification` (kostenlos) |
| Formel-Guide, Onboarding | `tutorial` |
| Auth, Convex | entfällt lokal; Account nur für Locora Cloud |
| Friends, Competitive, AI-Chat (im Schema, nicht live) | später eigene Plugins |

---

## 6. UI-Konzept: möglichst wenige Klicks

### 6.1 Startbildschirm = Plugin-Übersicht

```text
┌──────────────────────────────────────────────────────┐
│  locora                                  ⌘K Suche    │
│                                                      │
│  ▶ Weiter: „Planeten“ – 7 Loci fällig      [Enter]   │  ← 1 Klick zurück in die Arbeit
│                                                      │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ │
│  │  Core    │ │ Review   │ │ Tutorial │ │ Gamific. │ │
│  │ 12 Pal.  │ │ 7 fällig │ │ 3/8 ✓    │ │ 🔥 5 Tage│ │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘ │
│  ┌──────────┐                                        │
│  │ Notify 🔔│                                        │
│  │ 18:00 ⏰  │                                        │
│  └──────────┘                                        │
│  + Plugin hinzufügen                                 │
└──────────────────────────────────────────────────────┘
```

- Jede Kachel ist ein **Live-Widget** des Plugins (deklarativer UI-Beitrag), nicht nur ein Icon.
- Die **Weiter-Zeile** ist die wichtigste Aktion. Sie kommt aus `review` (fällige Loci) bzw. ohne `review` aus `core` (zuletzt bearbeitet).
- **Befehlspalette** (Strg/⌘+K, auf Mobile Wischen nach unten) durchsucht alle Befehle aller Plugins und alle Paläste.
- Einstellung „Direkt in Core starten“ für alle, die den Übersichtsschritt nicht wollen.

### 6.2 Core-Ansicht

Links bzw. oben steht die Palastliste, jede Karte mit **▶ (Journey starten)** direkt auf der Karte.
Es gibt keine Zwischenseite „Palast-Details“: Ein Klick auf die Karte öffnet sofort den Editor.

### 6.3 Klick-Budgets (verbindlich, werden als E2E-Tests geprüft)

| Aufgabe | mnemodim heute | Locora-Ziel |
|---|---|---|
| Journey für einen Palast starten | 3–4 | **1** (▶ auf Karte oder Enter auf „Weiter“) |
| Neuen Palast mit Bild anlegen | ~6 + Formulare | **1** (Bild/Video ins Fenster ziehen = Palast + Raum + Editor offen) |
| Locus platzieren und benennen | ~4 | **1 Klick + Tippen + Enter** (nächster Locus sofort bereit) |
| 10 Begriffe auf einmal als Loci | n × 4 | **Liste einfügen** (Strg+V) → Loci werden als Pfad vorplatziert, Ziehen zum Korrigieren |
| Video-Locus setzen | – | **Taste L** während Wiedergabe (+ optional Klick auf Bildposition) |
| Antwort in Journey | 1–2 | Recall: **Leertaste/Tap** zum Aufdecken, **1–4** bewerten; Quiz: Tasten **1–9** |

Weitere Regeln:

- Kein Speichern-Button: Alles speichert sofort (Op-Log). Rückgängig mit Strg+Z, auch auf Mobile per Schütteln oder Toast.
- Keine Modal-Kaskaden: Bearbeitung inline, Details in einem einklappbaren Seitenpanel.
- Jede Aktion hat eine Tastenkürzel- und eine Gesten-Variante.
- „Erweitert“ (Tabellen, Formeln) ist ausgeblendet, bis man es einmal aktiviert.

---

## 7. Video-Paläste

### 7.1 Modell

Ein Raum hat optional einen Hintergrund (`backdrop`); ist es ein Video, ist es ein Video-Raum. Ein Video-Locus hat
zusätzlich eine Zeit. Umgesetzt sind bereits `place`, `item`, `x`, `y`, `time` (siehe `format/README.md`); geplant sind:

```json
{
  "id": "l7",
  "place": "Tür",
  "item": "kleinster Planet",
  "x": 0.42, "y": 0.61,     // Marker auf dem Videobild
  "time": 83.40,            // Sekunden; 0 = „am Anfang“
  "pause": "untilSolved",   // geplant: "untilSolved" | "brief" (3 s) | "none" (nur Marker)
  "answer": { "type": "recall" }   // geplant: recall | quiz | guess
}
```

### 7.2 Ablauf in der Journey

1. Das Video läuft. Die Erkennung nutzt `requestVideoFrameCallback` (Fallback `timeupdate`).
   Erreicht die Wiedergabe `at`, wird **framegenau angehalten**, falls sie leicht darüber hinaus ist, zurück auf `at` gesprungen.
2. Der Locus erscheint als Overlay (an `pos` oder zentriert). Die Wiedergabe bleibt **gesperrt**, bis er gelöst oder bewusst übersprungen ist.
3. Ist der Locus gelöst, läuft das Video automatisch weiter. Es gibt keinen zusätzlichen „Weiter“-Klick, außer der Nutzer will die Lösung noch ansehen (dann Leertaste).
4. Mit `at: 0` erscheinen alle Loci **am Anfang** als Vorschau-Liste (Modus „Erst lernen, dann schauen“).
5. Spulen über ungelöste Loci ist in Journeys gesperrt, im Explore-Modus erlaubt.

### 7.3 Editor

Video abspielen und **L** drücken: Ein Locus wird beim aktuellen Zeitstempel gesetzt, das Eingabefeld hat sofort den Fokus.
Die Zeitleiste zeigt alle Loci als Punkte. Ziehen verschiebt den Zeitpunkt, **, / .** springen einzelne Frames.

### 7.4 Formate

Kanonisch ist **MP4 (H.264 + AAC)**. Das spielen alle WebViews ab: WKWebView, Android WebView, WebView2, WebKitGTK mit GStreamer.
Die CLI wandelt andere Formate per `ffmpeg` um, falls es installiert ist. Medien werden immer lokal gespeichert.
YouTube- oder Remote-URLs können später als Plugin kommen, verletzen aber „offline first“ und sind deshalb nicht im Kern.

---

## 8. CLI

Die CLI arbeitet auf derselben Datenbank wie die App. Änderungen erscheinen live in der offenen App, weil die App den Op-Log beobachtet.

```bash
# Palast in einem Befehl: Bild + Begriffe, Loci werden automatisch als Pfad verteilt
locora new "Planeten" --image wohnung.jpg -- Merkur Venus Erde Mars Jupiter

# Aus Datei (eine Zeile = ein Locus: "Inhalt", "Ort | Inhalt" oder "Ort | Inhalt | x,y")
locora new "Vokabeln" --image buero.jpg --from vokabeln.txt

# Video-Palast mit Zeitmarken
locora new "Vortrag" --video vortrag.mp4 -- 0:12=Einleitung 1:23=These 4:05=Fazit

# Bearbeiten
locora add Planeten "Saturn" --place Dachboden --at 0.7,0.3
locora edit Planeten 3 --item "Erde"
locora show Planeten
locora ls                      # Paläste (mit Fälligkeiten, falls `review` installiert)
locora practice Planeten       # Abfrage im Terminal (TUI, Text-Recall)
locora export Planeten -o planeten.locora
locora import planeten.locora

# Plugins
locora plugin ls
locora due                     # was ist heute fällig (aus `review`)
locora review                  # nur fällige Loci abfragen, in Routen-Reihenfolge
locora stats                   # kommt aus dem Gamification-Plugin, falls installiert
```

Zusätzlich gibt es ein menschenlesbares **Quelltext-Format** (`*.locora.toml`), das man in Git versionieren und mit
`locora build palast.locora.toml` zu einem Palast machen kann. Damit lassen sich auch Paläste mit KI oder Skripten erzeugen.

---

## 9. Dateiformat `.locora` (Version 1)

**Umgesetzt**, Spezifikation in [`format/README.md`](format/README.md), Referenz-Implementierung `crates/locora-format`.
`.mnemodim`-Dateien müssen nicht kompatibel bleiben.

- ZIP mit `mimetype` (erster Eintrag, unkomprimiert, `application/vnd.locora+zip`), `manifest.json`, `palaces/<id>.json`,
  optional `tables.json` (gemeinsame Tabellen), `media/<sha256>.<ext>`, reserviert `plugins/<id>/…`.
- camelCase-JSON; Dateinamen werden aus Ids/Hashes abgeleitet, nie doppelt gespeichert; Raum-Art ergibt sich aus dem
  Hintergrund-Medium. Ungültige Kombinationen sind damit nicht darstellbar.
- Deterministische Exporte (gleicher Inhalt = gleiche Bytes), strenges „fail closed“-Lesen mit Limits, Hash-Prüfung vor
  dem Schreiben, Import immer als neue Kopie; kollidierende gemeinsame Tabellenzeilen werden zu lokalen Zeilen.

---

## 10. Gamification und Bezahlung

- **Gamification ist kostenlos:** XP, Combos, Herzen, Trophäen und Daily Challenges laufen vollständig lokal und offline, ohne Account.
- **Locora Cloud kostet:** Ranglisten, Freunde, gemeinsame Challenges und Sync von Fortschritt und Palästen über Geräte.
- **Preis – bewusst günstig, damit man es sich „einfach mal holt“:**

  | Plan | Preis | Hinweis |
  |---|---|---|
  | **Jahresabo** (Hauptangebot) | **9,99 €/Jahr** (≈ 0,83 €/Monat) | eine Transaktion pro Jahr, Gebühren fallen kaum ins Gewicht |
  | Monatsabo | 1,49 €/Monat | für Unentschlossene; jederzeit kündbar |
  | Testphase | 14 Tage gratis | ohne Zahlungsdaten auf Desktop |

  Ein reines 1-€-Monatsabo lohnt sich nicht: Feste Gebühren pro Zahlung (ca. 0,30–0,50 €) würden 30–50 % davon auffressen.
- **App-Store-Regeln:** Auf iOS und Android müssen digitale Abos über In-App-Kauf laufen (Apple/Google nehmen 15 % im Small-Business-Programm).
  Auf Desktop und Web eignet sich ein Merchant of Record wie Paddle oder Lemon Squeezy (übernimmt EU-USt).
- **Server:** Rust (axum + Postgres) im privaten Repo `locora-cloud`. Er nutzt dieselben Crates (`locora-formula`, Journey-Replay) für fälschungssichere Ranglisten.

---

## 11. Roadmap

| Phase | Inhalt | Ergebnis |
|---|---|---|
| **0** | Repo, Cargo-Workspace, CI (Linux/macOS/Windows), Lizenzen, Entscheidungen aus Abschnitt 12 | leeres, baubares Gerüst |
| **1** | Plugin-Host mit wasmtime + WIT-SDK + Hot-Reload + `locora plugin new/dev` zuerst, dann `locora-store`, `locora-formula` (Port inkl. der bestehenden Testfälle aus `formulas.spec.ts` etc.), `locora-format`, `core` und `review` als WASM-Plugins ohne UI, **CLI** | Paläste per CLI anlegen, fällige Loci wiederholen, importieren und exportieren |
| **2** | Tauri-App: Startbildschirm, Befehlspalette, Core-Editor, Journey (Bild). **Android- und iOS-Builds ab Tag 1 in CI** | nutzbare App auf allen Plattformen |
| **3** | Video-Räume in der App: Editor + Journey; Plugin `notify` | Video-Paläste, Erinnerungen |
| **4** | Plugin `tutorial` (Touren über `ui.highlight`, Beispielpalast, Formel-Guide) | Onboarding |
| **5** | Plugin `gamification` (lokal, kostenlos), danach Locora Cloud: Server, Bezahlung, Entitlements | erstes Einkommen |
| **6** | Sync (Op-Log ↔ Server) als optionaler Dienst | mehrere Geräte |
| **7** | Plugin-Verzeichnis für Dritte, Signaturen, Store-Prüfung, Plugin-Tutorial und Vorlagen-Feinschliff | offenes Ökosystem |

Die Klick-Budgets aus 6.3 und die Startzeit werden ab Phase 2 als Tests in CI geprüft.

**Stand (16.10.2026):** Phase 0 erledigt, Phase 1 fast fertig. Details und alle offenen Aufgaben: Abschnitt 13.

---

## 12. Entscheidungen

| Thema | Entscheidung |
|---|---|
| Name | **Locora** |
| Sprache | **Pures Rust**, auch die UI (Tauri 2 + Leptos) |
| Lizenz der offenen Teile | **MIT** |
| Repository | `wasser-quest/locora` |
| Gamification | **kostenlos**, nur Locora Cloud kostet |
| Preis Locora Cloud | **9,99 €/Jahr** oder 1,49 €/Monat |
| `.mnemodim` | **keine Kompatibilität** nötig, neues Format `.locora` |
| Spaced Repetition | eigenes Plugin `review` (FSRS), kostenlos und MIT |
| Benachrichtigungen | eigenes Plugin `notify`, kostenlos und MIT |
| Grafiken | **Splinely** (`.sply` + Runtime) |
| Plugin-Technik | **zur Laufzeit geladene Rust → WASM-Komponenten** (wasmtime), Hot-Reload ohne Neustart |
| Fremde Plugins | erlaubt auf allen Plattformen; Android/iOS mit Warnung „gefährlich, auf eigene Verantwortung“, Desktop mit kurzer Warnung |
| Eigene Plugins | Nutzer können selbst Plugins bauen: Vorlagen, `locora plugin dev`, Hot-Reload, Test-Harness |
| Begriffe | **Palast → Räume → Loci**; ein Locus hat einen **Ort** (`place`) und einen **Inhalt** (`item`); Video-Zeit `time`; Tabellen (`table`) |
| Maskottchen | **Simo** (nach Simonides), Ganzkörper, ohne Konturen, Cel-Shading; dazu das Glühwürmchen |

---

## 13. Handout für den nächsten Agenten

Stand: 16.10.2026. Dieser Abschnitt ist die Übergabe: was es gibt, wie man arbeitet und was noch gebaut werden muss.

### 13.1 Arbeitsweise und Vorgaben des Nutzers

- **Kommunikation auf Deutsch.** Code, Kommentare, Commit-Messages und README auf Englisch; `PLAN.md`, `assets/STIL.md`
  und alle Texte, die Nutzer sehen (CLI, App), auf Deutsch.
- **Alles in Rust**, möglichst schnell. UI später mit Tauri 2 + Leptos.
- **Alle Grafiken mit `splinely-cli` als `.sply`** (Installation unter `~/.local/opt/splinely-v0.5.1-x86_64-linux/bin`,
  Version 0.5.2 ist als Release gebaut, aber noch nicht installiert). Für jede neue Idee mit Bildbedarf sofort eine `.sply`
  unter `assets/` anlegen, bevorzugt per Bauskript unter `assets/**/src/`; die Splinely-Runtime soll sie später rendern.
  Funktioniert in Splinely etwas nicht: dem Nutzer sagen. Splinely-Quellcode liegt in `~/svg_editor`
  (Repo `wasser-quest/splinely`); Fixes dort nur auf Wunsch, Releases per Tag `vX.Y.Z` (Version in `Cargo.toml` mitziehen).
- **Stil:** `assets/STIL.md` (ohne Konturen, Cel-Shading). Darin nie den Anime-Bezug oder Referenzbilder erwähnen.
- **Maskottchen Simo** (`assets/mascot/src/simo.py`) und das Glühwürmchen (`gluehwuermchen.py`). Der Nutzer will eine
  männlich wirkende Ganzkörperfigur; Mnemo wurde gelöscht.
- Lizenz MIT; Gamification kostenlos, nur Locora Cloud kostet (9,99 €/Jahr oder 1,49 €/Monat).
- Fremde Plugins: Warnung „gefährlich, auf eigene Verantwortung“ auf Android/iOS, kurze Warnung auf Desktop. Wird das
  Laden auf Mobile schwierig, darf es dort abgeschaltet werden.
- **Git:** Push per SSH (`~/.ssh/id_ed25519`, im Repo als `core.sshCommand` gesetzt). Commit-Messages enden mit
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Die GitHub-API meldet für das Repo derzeit „Not Found“
  (vermutlich privat); CI-Ergebnisse deshalb nur über die Weboberfläche oder mit Token sichtbar.

### 13.2 Was es schon gibt

| Baustein | Stand |
|---|---|
| `crates/locora-api` | Events (`core.*`, `host.started`, `ui.tile`), Services (`review.schedule@1`), API-Version 0.2 |
| `crates/locora-sdk` + `-macros` | `#[locora_plugin]`, `ctx.on`, `ctx.emit`, `ctx.provide`, `ctx.store()`, `ctx.tile`, WIT in `wit/plugin.wit` |
| `crates/locora-host` | wasmtime-Komponenten, Hot-Reload, Fuel-Limit, Absturz-Isolation, Kompilat-Cache, Berechtigungen (`storage`), Services, austauschbarer `KvStore` |
| `crates/locora-store` | SQLite (Migrationen 1–3), Paläste → Räume → Loci (`place`, `item`, `x`, `y`, `time`), Medien nach SHA-256, Tabellen (lokal/gemeinsam), Bindungen, Plugin-KV, Op-Log mit HLC |
| `crates/locora-fsrs` | FSRS-5 mit Standardgewichten |
| `crates/locora-formula` | vollständiger Port der mnemodim-Formelsprache + `workbook::Run` (Eingaben, Aktionen `set/reveal/reset`, Bindungen `place/item/x/y/visible/active`, `View.reveal`) |
| `crates/locora-format` | `.locora` v1 (siehe `format/README.md`) inkl. Tabellen und Bindungen |
| `plugins/review` | Spaced Repetition als WASM-Plugin; wird per `apps/cli/build.rs` gebaut und in die CLI eingebettet |
| `plugins/lernzeit`, `plugins/fixture` | Beispiel- bzw. Test-Plugin |
| `apps/cli` | `new/add/edit/ls/show/rm/practice/review/due/export/import`, `table show/set/rm`, `bind/unbind/calc`, `plugin new/dev/run/check`; Workbook-Eingaben, Aktivitätsfilter, Aufdecken und Aktionen in Practice/Review |
| `assets/` | `brand/icon.sply`, `mascot/simo.sply`, `mascot/gluehwuermchen.sply` (+ Exporte), `STIL.md` |

**Befehle:** `cargo test` (≈ 50 Tests, baut die Test-Plugins selbst), `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --all`; Plugins: `cd plugins && cargo clippy -- -D warnings`. Voraussetzung: `rustup target add wasm32-wasip2`
(steht in `rust-toolchain.toml`). `LOCORA_SKIP_PLUGINS=1` baut die CLI ohne eingebettete Plugins. Tests der CLI nutzen
`--data <tempdir>` und `LOCORA_NOW` für eine feste Uhrzeit.

**Stolpersteine:** wasmtime 49 braucht das Feature `anyhow`; rusqlite 0.40 braucht `fallible_uint` für `u64`;
zip 9: `file.name()` liefert ein `Result`; in Formeln `Major["7"]` statt `Major.7` (`.7` ist eine Zahl);
Splinely: `node duplicate` legt die Kopie direkt über das Original (für Boolean-Differenzen die verschobene Kopie
nach vorn holen), Gruppen sind erst ab 0.5.1 animierbar, `check` meldet bis 0.5.1 fälschlich State-Overrides als unsichtbar.

### 13.3 Offen in Phase 1 (Reihenfolge = Empfehlung)

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
   - `?` listet Aktionen; `!name` führt `set/reveal/reset` vor und nach dem Aufdecken aus, ohne zu bewerten oder
     weiterzugehen. Aktionen sind namensgetreu; nach Änderungen wird das aktuelle Feld neu angezeigt.
     `reset` löscht auch Eingabe-Overrides; Route bleibt der Start-Snapshot. Tabellen/Bindungen werden nie verändert.
     `x/y/visible` sind speicherbare Bindungen, haben aber im reinen Textterminal keine räumliche Darstellung;
     `visible=false` entfernt keinen Locus aus der Route.
   - Sechs Integrationstests in `apps/cli/tests/workbook.rs`: Bereiche/Overrides/Spalten/Typen, Calc/Bind/Unbind,
     Eingaben/Standards/EOF, Aktivität/Aufdecken, Aktionen/Fehler/Atomarität/Reset, Laufisolation und Review-Planung.
     Geprüft mit `cargo fmt --all`, `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`
     und zusätzlich Clippy im Plugin-Workspace. Nächster offener Schritt: Punkt 2.
2. **Hooks im Host** (Filter-Ketten aus 4.1, z. B. `core.journey.score`) und **Host-Services für Paläste**
   (`palaces:read`/`palaces:write`, bisher nur `storage` erzwungen).
3. **`core` als eigenes Plugin**: Journey-Regeln, Bewertung (Recall/Quiz/Guess), Hooks für Gamification. Dafür im Store
   Antwort-Arten für Loci ergänzen (mnemodim hatte Recall, Quiz mit mehreren richtigen Antworten, Guess).
4. **`locora plugin install/remove/ls`** mit kurzer Warnung auf Desktop, Prüfung von Rechten, Ablage in `<data>/plugins`;
   später Signaturen (Ed25519).
5. **`locora.toml`-Quelltextformat** + `locora build` (Paläste in Git pflegen, per KI erzeugen).
6. **`.locora` mit Fortschritt:** `locora export --with-progress` schreibt Review-Daten nach `plugins/review/`.
7. **Review-Log** (jede Antwort speichern) und FSRS-Parameteroptimierung; Tageslimits für neue Loci in den Einstellungen.
8. **Sammlungen** (mehrere Paläste mit gemeinsamer Navigation, `navigate`-Aktion aus mnemodim) – erst wenn gebraucht.

### 13.4 Phase 2: die App (Tauri 2 + Leptos)

- `apps/app/` anlegen: `src-tauri/` (Rust, nutzt `locora-store`, `locora-host`, `locora-format`, `locora-formula` direkt)
  und `ui/` (Leptos → WASM, `trunk`). Die UI enthält keine Fachlogik und ruft nur Tauri-Commands.
- **Startbildschirm** = Plugin-Übersicht mit Live-Kacheln (`ui.tile`-Events) und der „Weiter“-Zeile (fällige Loci aus
  `review.schedule@1`), Befehlspalette (Strg/⌘+K).
- **Core-Ansicht:** Palastliste mit ▶ direkt auf der Karte, Editor (Bild/Video ins Fenster ziehen = neuer Palast, Klick =
  Locus setzen, Liste einfügen = Loci als Route), Journey mit Tastatur (Leertaste, 1–4).
- **Splinely-Runtime einbinden:** Alle `.sply` aus `assets/` in der App abspielen. Optionen prüfen: die JS-Runtime aus
  `~/svg_editor/runtime/src/splinely.js` (kann binäre `.sply` dekodieren, `sply_decode.js`) in der WebView, oder die native
  Rust-Runtime `splinely_runtime` (Frames berechnen, Rendering trotzdem in der WebView). Doku: `RUNTIME_API.md`,
  `docs/native-runtime.md` im Splinely-Repo. Ziel: ein Leptos-Component `<Sply src="…"/>`.
- **Grafiken, die dafür als `.sply` entstehen müssen** (Stil: `assets/STIL.md`): Kachel-Icons für `core`, `review`,
  `notify`, `tutorial`, `gamification`; Raum-Arten (ohne Bild, Bild, Video); Locus-Marker (leuchtender Punkt mit Nummer,
  Zustände neu/fällig/gewusst); leere Zustände (keine Paläste, nichts fällig – Simo mit Laterne); Journey-Feedback
  (richtig, nochmal, geschafft); Splash/Ladezustand; Onboarding-Szenen für das Tutorial; Benachrichtigungs-Icon;
  später Trophäen und Streak-Flamme für Gamification.
- Android- und iOS-Builds ab Tag 1 in CI; Klick-Budgets (6.3) als E2E-Tests.

### 13.5 Danach (Phasen 3–7)

- **Phase 3:** Video-Räume in der App (anhalten am Locus bis gelöst, Editor mit Taste L, Zeitleiste), Plugin `notify`
  (4.6: lokale Benachrichtigungen, Ruhezeiten, Bündelung).
- **Phase 4:** Plugin `tutorial` (Touren über `ui.highlight@1`, Beispielpalast, Formel-Guide), Simo als Begleiter.
- **Phase 5:** Plugin `gamification` (XP, Combos, Herzen, Trophäen, Daily Challenges; lokal und kostenlos, Werte aus
  mnemodim `shared/journeyScoring.ts`), danach privates Repo `locora-cloud` (axum + Postgres, Abo, Ranglisten,
  Ed25519-Entitlements mit 14 Tagen Offline-Kulanz, In-App-Kauf auf iOS/Android).
- **Phase 6:** Sync über das Op-Log (HLC, Last-Writer-Wins pro Feld) mit Locora Cloud.
- **Phase 7:** Plugin-Verzeichnis, Signaturen, WASM-Plugins für Dritte inkl. deklarativer UI (4.4) und Store-Prüfung.
