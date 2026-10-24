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
Tutorial, leere Zustände, Erfolge (Gamification) und Benachrichtigungen. Stil-Leitfaden: [`assets/STIL.md`](../assets/STIL.md). App-Icon: Palast-Torbogen mit leuchtenden Loci und dem Licht des Glühwürmchens.

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
│   └── gamification/            XP für fällige Wiederholungen und neue Orte, Wochenziel, Trophäen, Palastmedaillen
│                                (lokal, kostenlos; über Locora Cloud auf allen Geräten gleich, siehe 4.7)
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
| **Hooks** (Filter-Ketten) | Plugins dürfen ein Ergebnis verändern | `core.journey.score` → ein Punkte-Plugin vergibt Bonuspunkte |
| **Storage** | Eigener Namensraum in SQLite (KV), wird gesynct; `storage.list@1` liefert, was sich seit einer Revision geändert hat | `gamification` → `g/<epoche>/day/<datum>/<gerät>` |
| **UI-Beiträge** | Deklarativ: Kachel, Befehle, Panels, Locus-Typen, Overlays | Gamification zeigt XP, Tagesziel und Woche als Kachel |
| **CLI-Beiträge** | Eigene Unterbefehle | `locora tutorial reset`, `locora stats` |

Payloads sind serde-Structs. Sie werden im SDK als Typen veröffentlicht und intern als JSON/Postcard übertragen.
Dieselben Typen gelten auf beiden Seiten der WASM-Grenze.

### 4.2 So reden die Plugins miteinander

```text
core ──event: journey.reviewed ───────▶ gamification (fällig/neu/zu früh/heute schon, Lerntag, Gerät, Epoche)
core ──hook:  journey.score ──────────▶ Punkte-Plugins (Bonus) ──▶ zurück an core
core ──event: palace.created ─────────▶ tutorial (nächster Tour-Schritt)
core ──event: journey.locus_answered──▶ review (FSRS-Bewertung, neuer Fälligkeitstermin)
core ──hook:  journey.roster ─────────▶ review (nur fällige Loci abfragen)
core/host ──service: review.due@1 ────▶ review (Badges auf Palastkarten, Weiter-Zeile)
review ──service: notify.schedule@1 ──▶ notify (Erinnerung „12 Loci fällig“)
host ──service: gamification.status@2 ▶ gamification (heute offen Fälliges, Palaststärke → Tagesziel, Medaillen)
host ──service: gamification.reset@1 ─▶ gamification (neue Epoche: alter Fortschritt wird geleert)
tutorial ──service: core.palaces@1 ───▶ core (Beispielpalast anlegen)
tutorial ──service: ui.highlight@1 ───▶ host (Element hervorheben)
gamification ──event: trophy.unlocked ▶ tutorial (erklärt Trophäen beim ersten Mal)
```

Gamification schickt **keine** Benachrichtigungen: Erinnerungen gibt es nur, wenn man sie selbst einschaltet
(„12 Loci fällig“, über `review` und `notify`).

Alle Abhängigkeiten außer `core` sind **optional**. Fehlt Gamification, rechnet `core` die Journey
einfach ohne Hook. Fehlt `review`, gibt es keine Fälligkeiten und die Weiter-Zeile zeigt den zuletzt bearbeiteten Palast. Fehlt Tutorial, passiert gar nichts.

### 4.3 Open Core: was ist offen, was kostet

| Teil | Lizenz | Preis |
|---|---|---|
| Host, SDK, Store, Format, Formula, CLI | MIT | frei |
| Plugin `core` | MIT | frei, voller Funktionsumfang offline |
| Plugin `tutorial` | MIT | frei |
| Plugin `gamification` | MIT | frei, XP/Trophäen/Challenges lokal |
| **Locora Cloud** (Server) | proprietär | Abo: Sync über Geräte (Ende-zu-Ende-verschlüsselt); die öffentliche Rangliste bleibt kostenlos |

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

Die offiziellen Plugins (`core`, `review`, `notify`, `tutorial`, `gamification`) kommen mit der App.
Ed25519-Signaturen sind für die Plugin-Pakete vorgesehen und noch nicht umgesetzt.
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
  Eine Abweichung von FSRS-5 (`crates/locora-fsrs`): Eine Antwort weniger als einen Tag nach der vorigen hebt die
  Stabilität nie (ein „Nochmal“ oder „Schwer“ senkt sie weiter). Sonst würde zehnmal Üben an einem Abend einen Ort
  Monate wegplanen: Pauken darf den Plan nicht verschieben.
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
  - Jede Nachricht hat Plugin, Kanal (z. B. `review.due`), Priorität und eine **Aktion**
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
  („12 Loci fällig · 3 Paläste“). Jeder Kanal lässt sich einzeln abschalten.
- **Ohne `notify`:** Plugins laufen weiter, Hinweise erscheinen nur als Badges in der App.
- **CLI:** `locora notify ls`, `locora notify mute <kanal>`, `locora notify quiet 22:00-08:00`.

### 4.7 Plugin `gamification`: Fortschritt, der dem Lernen dient

Die Regeln des Spiels (seit 0.2.0 des Plugins). Alles läuft offline, ohne Konto und kostenlos; es gibt keine Preise,
keine Käufe, keine Benachrichtigungen und keine Verlust-Sprache.

- **XP pro Ort und Lerntag, egal wie bewertet:** 10 für eine fällige Wiederholung, 6 für einen neu gelernten Ort,
  einmal am Tag +20, wenn an dem Tag fällige Wiederholungen anstanden und alle erledigt sind. Früheres Üben (noch
  nicht fällig) ist erlaubt und bringt 0 XP. Ein Ort zählt am selben Lerntag nur einmal, auch über Geräte; was heute
  schon dran war, ist beim nächsten Mal Nachüben („Nochmal, zehn Minuten warten, nochmal“ bringt nichts). Wer ehrlich
  „Nochmal“ wählt, verliert nichts. Journeys selbst bringen keine XP mehr (`locora-scoring` bleibt nur für die
  Ranglisten von Locora Cloud).
- **Keine Herzen.** Die Combo („3 in Folge“) gibt es nur bei Quiz und Guess, ab zwei richtigen Antworten und ohne
  XP-Wirkung; eine Recall-Bewertung zeigt und bricht sie nicht. Nach einem Fehler steht ein ruhiges „Kommt bald
  wieder – so bleibt es hängen.“
- **Fällig ist, was an diesem Lerntag fällig wird:** eine Wiederholung ist den ganzen Lerntag fällig, auf den ihr
  Termin fällt, egal ob morgens oder abends gelernt wird (`DueDay` in `crates/locora-runtime/src/game.rs`; dieselbe
  Regel für Tagesziel, Startseite, fällige Journeys und XP). Orte, die heute schon dran waren und nach „Nochmal“
  zurückkommen, sind Nachüben: Die fällige Journey fragt sie, sobald ihre Zeit da ist, sie zählen aber nicht zum
  Tagesziel und bringen keine XP; Leiste und Abschlusskarte sagen das („zum Nachüben“, „Kommt heute noch einmal“).
- **Tagesziel = das Fällige:** erledigt sind die fälligen Wiederholungen des Tages (über alle Geräte), offen ist, was
  heute fällig und heute noch nicht wiederholt ist. Bei einem großen Rückstand gilt das Ziel nach 50 als erreicht. Ist
  nichts fällig, zeigt die Leiste „Heute ist nichts fällig“ ohne Fortschrittsbalken; dafür gibt es weder +20 noch die
  Trophäe „Alles Fällige erledigt“.
- **Wochenrhythmus statt Tagesserie:** Ziel 1–7 Lerntage pro ISO-Woche (Standard 4), selbst gewählt. Ein Lerntag ist
  ein Tag mit fälliger oder neuer Wiederholung auf irgendeinem Gerät; die App nur zu öffnen zählt nicht. Ein Tag, an
  dem nichts fällig war und nichts gelernt wurde, senkt das Ziel seiner Woche um einen Tag (auf mindestens einen
  Lerntag). Solche Tage erkennt das Plugin am Wiederholungsplan, nicht an einem Besuch: Bei jedem Blick auf den Stand
  merkt sich ein Gerät, wann als Nächstes etwas fällig ist („look“); die Tage davor waren frei, auch wenn die App an
  ihnen geschlossen blieb. Eine Woche ohne Lerntag, deren freie Tage das Ziel abdecken, zählt nicht und bricht keine
  Serie. Angezeigt werden die Lerntage insgesamt (sie sinken nur durch Löschen), „diese Woche x von y“ (y = Ziel der
  Woche, x höchstens y) mit den Tagen ohne Fälliges und, nachrangig, wie viele Wochen in Folge das Ziel erreicht wurde;
  eine laufende Woche bricht das nicht, der Bestwert bleibt. Nach einer verpassten Woche: „Neue Woche, neues Ziel.“
- **Trophäen**, alle mit festem Datum: erste Journey, 100 XP, 7 Lerntage (nicht in Folge), alles Fällige an einem Tag,
  Wochenziel 4-mal. „Fehlerfreie Journey“ bleibt nur, wo sie schon verdient war. Freie Tage zählen bei keiner.
- **Palastmedaillen aus gezeigter Stabilität** (Paläste ab 5 Orten): Verankert (9 von 10 Orten halten 7 Tage),
  Gefestigt (30 Tage), Dauerhaft (180 Tage). Ein Ort zählt nur, wenn er an einem späteren Lerntag als seinem ersten
  richtig (mindestens „Schwer“) beantwortet wurde, und nur mit der Stabilität, die das Antwortlog mit der jeweils
  ersten Antwort jedes Lerntags ergibt (höchstens der seiner Karte): Pauken an einem Tag bringt keine Medaille, auch
  kein erstes „Leicht“. Außerdem hebt eine Antwort am selben Tag die FSRS-Stabilität nicht mehr (4.5), sodass frühes
  Üben die nächste Wiederholung nicht Monate wegschiebt. Eine Medaille bleibt, auch wenn die Stärke später sinkt.
  Jede Palastkarte zeigt „Gefestigt: x von y Orten“ nach derselben Regel.
- **Feiern** nur, was dieses Gerät gerade verdient hat: Das Plugin merkt sich in der Sitzung, was die eigenen
  Antworten und die eigene Journey freigeschaltet haben (`SessionView::unlocked`); Synchronisiertes erscheint still.
  Gleichzeitig Verdientes wird nacheinander gefeiert; auf dem Handy oben, damit die Karte keine Knöpfe verdeckt.
  Medaillen stehen mit Datum unter „Erfolge“; mit reduzierter Bewegung bleiben sie still. Wörter: Trophäen sind die
  der Journeys, Medaillen die der Paläste, Erfolge die der App (Lektionen, Tour, Tutorial).
- **Die Erklärung** („So funktionieren XP“, für Umsteiger „… jetzt“) erscheint einmal, nie während der App-Tour oder
  in Simos Beispielpalast, und ist auf dem Handy zu einer Zeile eingeklappt.

**Daten: Fakten statt Zähler.** Jedes Gerät schreibt nur eigene Schlüssel und ergänzt sie nur:
`g/<epoche>/day/<YYYY-MM-DD>/<gerät>` (sortierte FNV-1a-Hashes der fälligen und neuen Orte, Journeys, Flag „alles
erledigt“, der letzte Blick auf den Plan: wann und welcher Lerntag als Nächstes fällig ist), `g/<epoche>/medal/<palast>/<stufe>/<gerät>`
(Tag), einmalig `legacy/<gerät>` (der alte, synchronisierte Stand) und, im Browser, `legacy/<gerät>/own` (was der
Browser vor dem Sync nur für sich in IndexedDB hatte), `pref/week-goal/<YYYY-Www>` und `seen/<hinweis>`. Der Sync
führt Schlüssel per Last-Writer-Wins zusammen; weil der neueste Wert eines Schlüssels jeden früheren enthält, geht
nichts verloren und nichts zählt doppelt (vom Blick auf den Plan zählt je Tag der jüngste aller Geräte). XP,
Lerntage, freie Tage, Wochen und Trophäen leitet das Plugin aus den zusammengeführten Fakten ab
(`plugins/gamification/src/ledger.rs`). Der alte, geteilte Stand (`progress`) zählt mit seinem größten Wert, nie als
Summe; der eigene Stand eines Browsers kommt dazu. Die Laufzeit liefert Lerntag, Gerät und Epoche
(`core.journey.reviewed`, `gamification.status@2`), das Plugin braucht keine Uhr. Eine Epoche übernimmt das Plugin
nur aus Anfragen der Laufzeit, nie aus einem Ereignis (das jedes Plugin senden könnte).

**App-Erfolge** (Lektionen von „Mnemotechnik lernen“, Tutorial, Formel-Guide, App-Tour, erster eigener Palast) sind
Lernfortschritt, kein Spiel: Jedes Gerät schreibt `achievements/<id>/<gerät>` mit dem Tag (und `achievements/<id>` = `1`
für ältere Versionen); es gilt der früheste Tag aller Geräte. Löschen des Spielfortschritts lässt sie stehen.

**Löschen:** „Spielfortschritt löschen“ in den Einstellungen schreibt eine neue Epoche (`host/game-epoch`, die
letzte gewinnt). Nur Fakten der aktuellen Epoche zählen; jedes Gerät leert ältere, sobald es die neue sieht, auch
wenn ein Gerät nach dem Löschen noch alte Fakten nachliefert. Der Tag des Löschens ist nie ein freier Tag (seine
Wiederholungen sind mitgelöscht). Wiederholungsplan, Paläste, App-Erfolge und Einstellungen bleiben. Ist das Gerät
angemeldet, bittet es Locora Cloud, die Ranglistenpunkte zu löschen (`DELETE /v1/game/data`, siehe `docs/CLOUD.md`);
ohne Bestätigung fragt es bei Start, Anmeldung, Statusabfrage und Sync wieder, auch ohne Sync-Zugang.

### 4.8 Plugin `competition`: Gedächtnis-Wettkampf

Die Regeln stehen in `crates/locora-competition` und gelten gleich in App, CLI und Locora Cloud: Generator, Wertung,
Challenge-Codes, Wochenplan und die Wortliste `groceries-v1`. Das Plugin speichert Runden und leitet alles andere
daraus ab. Alles läuft offline, ohne Konto und kostenlos.

- **Drei Disziplinen in drei festen Stufen:** Einkaufsliste (10, 20 oder 30 Dinge, in beliebiger Reihenfolge
  abzurufen), Zahlen (20, 40 oder 80 Ziffern in Zehnerreihen: eine Reihe ohne Fehler bringt 10 Punkte, mit einem
  Fehler 5, sonst 0) und Karten (13, 26 oder 52, ein Punkt je Karte am richtigen Platz). Jede Stufe hat feste Zeiten
  zum Einprägen und Abrufen; bei gleichen Punkten entscheidet die kürzere Einprägezeit, nie die Abrufzeit.
- **Arten von Runden:** Übung (ein neuer Seed oder die Aufgabe eines der letzten sieben Tage), die Tages-Challenge
  (Wochenplan: Montag und Donnerstag Einkaufsliste, Dienstag und Freitag Zahlen, Mittwoch und Samstag Karten, Sonntag
  ein „langer Tag“ im Wechsel; alle drei Stufen, je Stufe ein Versuch pro Lerntag) und Challenge-Codes (Disziplin,
  Stufe und Seed, z. B. `C2-28T5-CY4T-QKFF-K`: wer denselben Code spielt, bekommt dieselbe Aufgabe). Übung ohne
  Zeitlimit zählt weder für Rekorde noch für Trophäen.
- **Abendabfrage:** Jede Tagesrunde lässt sich ab sechs Stunden nach der Abgabe bis einen Tag danach noch einmal
  abrufen, ohne zweiten Blick. Sie ist persönlich und kommt in keine Rangliste.
- **Rekorde** je Disziplin und Stufe zählen nur das erste Spiel einer Aufgabe (auf irgendeinem Gerät) mit Zeitlimit.
  Schon das Zeigen zählt: Auch eine Runde, die nie abgegeben wird, hat ihre Aufgabe gesehen. Ob eine Aufgabe neu war,
  entscheidet das Gerät beim Start und speichert es mit (`first`), damit eine zurückgestellte Uhr keine Wiederholung
  zum ersten Spiel macht; die früheste Startzeit entscheidet nur zwischen Geräten, die dieselbe Aufgabe offline zum
  ersten Mal gesehen haben. Acht Trophäen (`compete-*`), jede mit dem Lerntag, an dem sie zuerst verdient wurde.
  Gezählt werden Tage mit Tages-Challenge insgesamt, nie als Serie.
- **XP fürs Mitmachen, nicht für Punkte:** 10, 15 oder 20 je Stufe der Tages-Challenge, 5 für die Abendabfrage, für
  Übung und Codes nichts, und nur für eine Abgabe mit mindestens einer Antwort (`locora_competition::xp`). Die
  Laufzeit meldet sie an `gamification` (`gamification.contest@1`, noch offen); ein Lerntag entsteht dadurch
  nicht.

**Daten: Fakten statt Zähler,** wie bei `gamification`, aber als JSON mit Version `v` (eine ältere Version liest die
Fakten einer neueren und überliest unbekannte Felder), unter der Spiel-Epoche der Laufzeit: `g/<epoche>/r/<runde>`
(die abgegebene Runde: Seed, Antworten, Punkte, Zeiten, Lerndatum, Sprache, Route),
`g/<epoche>/s/<datum>/<disziplin>/<stufe>/<runde>` (die Startmarke einer Tagesrunde, beim Start geschrieben),
`g/<epoche>/t/<disziplin>/<stufe>/<seed>/<runde>` (die Sichtung jeder anderen Runde, ebenfalls beim Start) und
`g/<epoche>/e/<runde>/<abendabfrage>`. Runden-Ids sind UUID v7 (32 Hex-Zeichen), Seeds 60 Bit aus dem Zufall des
Systems. Jeder Schlüssel wird einmal geschrieben, der Sync vereinigt die Geräte. Runden und Abendabfragen zählen nur
so, wie das Plugin sie aus dem Seed neu wertet, nie mit den gespeicherten Punkten; was kein Gerät dieser Version
geschrieben haben kann (andere Regeln, Zeiten über dem Limit, eine Tagesrunde mit fremder Aufgabe, Antworten, die
nicht zur Aufgabe passen, eine Abendabfrage außerhalb ihres Fensters), bleibt außen vor. Eine Runde mit einem späteren
Lerndatum als heute (die Uhr ihres Geräts ging vor) zählt für Rekorde, Trophäen und Tage mit Tages-Challenge erst ab
ihrem Datum, und ihre Abendabfrage erscheint nicht vorher. Haben zwei Geräte dieselbe Tages-Challenge offline
gespielt, ist die Runde mit der frühesten Startmarke der Versuch des Tages, die andere zählt als Übung, auf beiden
Geräten gleich. Eine Startmarke zählt allein durch ihren Schlüssel: Auch eine Marke einer Art, die diese Version nicht
kennt (Locora Clouds Tages-Challenge schreibt `cloud_daily` unter dieselben Schlüssel), oder eine, deren Wert sich
nicht lesen lässt, ist ein Versuch und belegt die Stufe; `today@1` und `begin@1` lesen dieselben Marken. Runden einer
Art, die 0.1.8 nicht kennt (`cloud_daily`, `duel`), überliest es dagegen absichtlich und zeigt ihre Stufe als
„begonnen, nicht beendet“; Locora Cloud muss damit rechnen. Eine begonnene und nie abgegebene Tagesrunde bleibt
„begonnen, nicht beendet“: Dieselbe Aufgabe ein zweites Mal zu sehen, wäre kein Versuch mehr. Das Plugin hält die
Fakten in einem Speicherabbild, das es über `storage.list@1` nachführt, und berechnet daraus den Stand von heute,
Rekorde, Trophäen und den Verlauf (`plugins/competition/src/facts.rs`). Die laufende Runde liegt nur im App-Kern;
nichts Halbfertiges wird synchronisiert.

**Epoche und Löschen:** Jede Anfrage bringt Epoche, Lerndatum und Uhrzeit der Laufzeit mit. Das Plugin hat keine Uhr
und übernimmt eine Epoche nur aus diesen Anfragen. „Spielfortschritt löschen“ ruft auch `competition.reset@1` auf:
Runden, Rekorde und Trophäen älterer Epochen sind leer, auch was ein Gerät später noch nachliefert. Ein eigenes
„Verlauf löschen“ gibt es nicht.

**Dienste** (`locora-api::competition`, mit den Typen der Regel-Crate): `competition.today@1`, `begin@1`,
`finish@1`, `evening@1`, `evening_finish@1`, `records@1`, `history@1` und `reset@1`. Ereignisse gibt es keine:
XP aus einem Ereignis ließe sich fälschen, denn jedes Plugin kann jedes Thema senden. Die Kachel „Gedächtnis-Wettkampf“
zeigt den Stand von heute. Im Browser liegt der Speicher im Profil des Kerns und wird synchronisiert wie auf dem
Desktop (`SYNCED_PLUGINS`).

**Laufzeit, App-Kern und CLI:** `locora_runtime::competition` vergibt Runden-Ids und Seeds und ruft die Dienste mit
Epoche, Lerndatum (`Profile::learning_day`) und Uhrzeit auf; „Spielfortschritt löschen“ ruft auch
`competition.reset@1`. Der App-Kern (`locora_app_core::competition`) hält die eine laufende Runde im Speicher: Die
Einprägezeit zählt bis zu ihrem Limit, die Abrufzeit bis zu ihrem Limit plus 10 Sekunden; eine abgebrochene
Tagesrunde bleibt begonnen. Er bietet die Befehle `compete_*` für Tauri und den Browser, nennt die Paläste, deren
Route für eine Runde reicht (die eingebauten nicht), und beschriftet die Orte nach „pro Ort“. Im Terminal:
`locora compete` (`today`, `play`, `evening`, `code`, `records`, `history`); eine Runde zeigt die Aufgabe auf dem
Wechselbildschirm, damit der Verlauf des Terminals nichts behält.

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
| Timing-Punkte, Combos, XP, Trophäen, Daily Challenges | `gamification` (kostenlos; XP für fällige und neue Orte, Wochenziel statt Herzen und Tagesserie, siehe 4.7) |
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

**Umgesetzt**, Spezifikation in [`format/README.md`](../format/README.md), Referenz-Implementierung `crates/locora-format`.
`.mnemodim`-Dateien müssen nicht kompatibel bleiben.

- ZIP mit `mimetype` (erster Eintrag, unkomprimiert, `application/vnd.locora+zip`), `manifest.json`, `palaces/<id>.json`,
  optional `tables.json` (gemeinsame Tabellen), `media/<sha256>.<ext>`, reserviert `plugins/<id>/…`.
- camelCase-JSON; Dateinamen werden aus Ids/Hashes abgeleitet, nie doppelt gespeichert; Raum-Art ergibt sich aus dem
  Hintergrund-Medium. Ungültige Kombinationen sind damit nicht darstellbar.
- Deterministische Exporte (gleicher Inhalt = gleiche Bytes), strenges „fail closed“-Lesen mit Limits, Hash-Prüfung vor
  dem Schreiben, Import immer als neue Kopie; kollidierende gemeinsame Tabellenzeilen werden zu lokalen Zeilen.

---

## 10. Gamification und Bezahlung

- **Gamification ist kostenlos:** XP, Tagesziel, Wochenziel, Trophäen und Palastmedaillen laufen vollständig lokal und offline, ohne Account (Regeln: 4.7).
- **Locora Cloud kostet:** Sync von Fortschritt und Palästen über Geräte. Die Rangliste (eine öffentliche, Tag/Woche/Monat/Jahr, nur auf Wunsch) bleibt kostenlos; Freunde bleiben; gemeinsame Challenges der Journeys können entfallen.
- **Preis (Entscheidung vom 22.10.2026):**

  | Plan | Preis | Hinweis |
  |---|---|---|
  | Monatsabo | **5 €/Monat** | jederzeit kündbar |
  | Jahresabo | **48 €/Jahr** | |
  | Testzeitraum | sechs Monate | jedes neue Konto ist ein Gastkonto und synchronisiert in dieser Zeit ohne Abo |

- **Bezahlen nur im Web:** Paddle als Merchant of Record (übernimmt die EU-USt). Kein In-App-Kauf: Die nativen Apps
  zeigen keine Preise, keine Kaufknöpfe und keine Links zu einer Kasse (Anti-Steering-Regeln von Apple und Google);
  `apps/app/ui/src/store_rules.rs` prüft das. Die Apps nennen höchstens neutral den Stand („Abo: aktiv“).
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

**Stand (18.10.2026):** Phasen 0 und 1 sind abgeschlossen. Die App, Video-Journeys,
Tutorial-Palast, erste App-Tour, Formel-Guide und lokale Gamification sind umgesetzt.
Offene Arbeiten und native Prüfungen stehen in [`OFFEN.md`](OFFEN.md).

**Stand (22.10.2026):** Veröffentlicht ist 0.1.3 (App-Tour mit Simo, Ton pro Locus, fünf Sprachen); 0.1.4
(Erklärungen ⓘ an den Feldern, übersetzte Texte) ist vorbereitet, aber noch nicht getaggt. Der verschlüsselte Sync (Phase 6) läuft in Desktop, Mobil, Browser und CLI, mit
Auto-Sync, E-Mail-Bestätigung und Passwort-Reset; siehe [`CLOUD.md`](CLOUD.md). Als Nächstes: Deploy des Servers,
die neue Rangliste, dann Bezahlen im Web.

---

## 12. Entscheidungen

| Thema | Entscheidung |
|---|---|
| Name | **Locora** |
| Sprache | **Pures Rust**, auch die UI (Tauri 2 + Leptos) |
| Lizenz der offenen Teile | **MIT** |
| Repository | `wasser-quest/locora` |
| Gamification | **kostenlos**, nur der Sync über Locora Cloud kostet |
| Preis Locora Cloud | **5 €/Monat** oder **48 €/Jahr**, nur im Web (Paddle), nie in den Apps |
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

Stand: 17.10.2026. Dieser Abschnitt enthält auch historische Implementierungsschritte.
Für den aktuellen Arbeitsstand gilt [`OFFEN.md`](OFFEN.md).

### 13.1 Arbeitsweise und Vorgaben des Nutzers

- **Kommunikation auf Deutsch.** Code, Kommentare, Commit-Messages und README auf Englisch; `PLAN.md`, `assets/STIL.md`
  und alle Texte, die Nutzer sehen (CLI, App), auf Deutsch.
- **Alles in Rust**, möglichst schnell. Die UI nutzt Tauri 2 + Leptos.
- **Alle Grafiken mit `splinely-cli` als `.sply`** (Installation unter `~/.local/opt/splinely-v0.5.2-x86_64-linux/bin`,
  Version 0.5.2, installiert am 16.10.2026). Für jede neue Idee mit Bildbedarf sofort eine `.sply`
  unter `assets/` anlegen, bevorzugt per Bauskript unter `assets/**/src/`; die Splinely-Runtime soll sie später rendern.
  Funktioniert in Splinely etwas nicht: dem Nutzer sagen. Splinely-Quellcode liegt in `~/svg_editor`
  (Repo `wasser-quest/splinely`); Fixes dort nur auf Wunsch, Releases per Tag `vX.Y.Z` (Version in `Cargo.toml` mitziehen).
- **Stil:** `assets/STIL.md` (ohne Konturen, Cel-Shading). Darin nie den Anime-Bezug oder Referenzbilder erwähnen.
- **Maskottchen Simo** (`assets/mascot/src/simo.py`) und das Glühwürmchen (`gluehwuermchen.py`). Der Nutzer will eine
  männlich wirkende Ganzkörperfigur; Mnemo wurde gelöscht.
- Lizenz MIT; Gamification kostenlos, nur Locora Cloud kostet (seit 22.10.2026: 5 €/Monat oder 48 €/Jahr, nur im Web).
- Fremde Plugins: Warnung „gefährlich, auf eigene Verantwortung“ auf Android/iOS, kurze Warnung auf Desktop. Wird das
  Laden auf Mobile schwierig, darf es dort abgeschaltet werden.
- **Git:** Push per SSH. Commit-Messages folgen den Repository-Vorgaben; kein `Co-Authored-By`-Trailer
  ohne ausdrückliche Autorisierung. Das Repository ist privat; CI-Ergebnisse benötigen GitHub-Zugriff.

### 13.2 Was es schon gibt

| Baustein | Stand |
|---|---|
| `crates/locora-api` | Typisierte Events, Services und Hooks für Core, Review, Notify, Tutorial und Gamification; API-Version 0.7 (0.6-Komponenten laufen weiter) |
| `crates/locora-sdk` + `-macros` | `#[locora_plugin]`, `ctx.on/emit/provide/filter/call_host/palaces`, `ctx.store()`, `ctx.tile`, WIT in `wit/plugin.wit`; Anleitung: `crates/locora-sdk/README.md` |
| `crates/locora-host` | wasmtime-Komponenten, Hot-Reload, Fuel-Limit, Absturz-Isolation, Kompilat-Cache, Berechtigungen (`storage`, `palaces:read/write`), Filter-Ketten, native Palastdienste, austauschbarer `KvStore` |
| `crates/locora-store` | SQLite (Migrationen 1–12), Paläste → Räume → Loci, Medien nach SHA-256, Tabellen, Bindungen, Plugin-KV, Op-Log mit HLC, Sync (Feldzeiten, Grabsteine, Anwenden von Remote-Ops), Journeys |
| `crates/locora-scoring` | Journey-XP-Regel und Journey-Replay (Plugin `gamification` und Locora Cloud) |
| `crates/locora-competition` | Gedächtnis-Wettkampf: Regeln, Generator, Wertung, Challenge-Codes und die Wortliste `groceries-v1` (Plugin `competition` und Locora Cloud) |
| `crates/locora-cloud-api` | Typen der Locora-Cloud-API und signierte Entitlements (Ed25519, 14 Tage Kulanz) |
| `crates/locora-signing` | Ed25519-Signaturen für Plugins (`<datei>.sig`) |
| `locora-runtime`: `cloud`, `cloud_sync`, `publishers` | Sitzung und Freischaltung, Sync mit Locora Cloud, vertrauenswürdige Plugin-Herausgeber |
| `locora-cloud` (eigenes privates Repo `wasser-quest/locora-cloud`) | Konten, Abo-Webhook, Ranglisten, Freunde, Challenges, Sync-Server, Medien; Tests gegen PostgreSQL |
| `crates/locora-fsrs` | FSRS-5 mit Standardgewichten |
| `crates/locora-formula` | vollständiger Port der mnemodim-Formelsprache + `workbook::Run` (Eingaben, Aktionen `set/reveal/reset`, Bindungen `place/item/x/y/visible/active`, `View.reveal`) |
| `crates/locora-format` | `.locora` v1/v2 (siehe `format/README.md`) inkl. Tabellen, Bindungen und Antwortarten; Recall-only bleibt v1 |
| `plugins/core`, `plugins/review` | Bewertung/Basispunkte bzw. Spaced Repetition als WASM-Plugins; beide per `apps/cli/build.rs` in die CLI eingebettet |
| `plugins/lernzeit`, `plugins/fixture`, `plugins/palace-reader`, `plugins/palace-writer`, `plugins/malformed`, `plugins/lifecycle` | Beispiel- bzw. WASM-Test-Plugins für Ereignisse, Rechte, Filter und Lebenszyklus |
| `apps/cli` | `new/add/edit/ls/show/rm/practice/review/due/export/import`, `table show/set/rm`, `bind/unbind/calc`, `plugin new/dev/run/check`; Workbook-Eingaben, Aktivitätsfilter, Aufdecken und Aktionen in Practice/Review |
| `assets/` | `brand/icon.sply`, `mascot/simo.sply`, `mascot/gluehwuermchen.sply` (+ Exporte), `STIL.md` |

**Befehle:** `cargo test --workspace` (74 Tests plus Doctests, baut die Test-Plugins selbst),
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`;
Plugins: `cd plugins && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`.
Native Plugin-Tests: `cd plugins && cargo test --workspace --target x86_64-unknown-linux-gnu` (2 Core-Regeltests;
auf anderen Plattformen natives Target anpassen). Voraussetzung: `rustup target add wasm32-wasip2`
(steht in `rust-toolchain.toml`). `LOCORA_SKIP_PLUGINS=1` baut die CLI ohne eingebettete Plugins. Tests der CLI nutzen
`--data <tempdir>` und `LOCORA_NOW` für eine feste Uhrzeit.

**Stolpersteine:** wasmtime 49 braucht das Feature `anyhow`; rusqlite 0.40 braucht `fallible_uint` für `u64`;
zip 9: `file.name()` liefert ein `Result`; in Formeln `Major["7"]` statt `Major.7` (`.7` ist eine Zahl);
Splinely: `node duplicate` legt die Kopie direkt über das Original (für Boolean-Differenzen die verschobene Kopie
nach vorn holen), Gruppen sind erst ab 0.5.1 animierbar, `check` meldet bis 0.5.1 fälschlich State-Overrides als unsichtbar.

### 13.3 Phase 1: Stand und offene Punkte

Ausführliche Verträge, Randfälle und Grenzen: [`docs/phase-1-notizen.md`](phase-1-notizen.md).

| # | Punkt | Stand |
|---|---|---|
| 1 | Workbook in der CLI: `table show/set/rm`, `bind/unbind`, `calc`, Eingaben und Aktionen beim Üben | erledigt |
| 2 | Hook-Ketten im Host, nativer Dienst `core.palaces@1` mit `palaces:read`/`palaces:write` | erledigt (API 0.3) |
| 3 | `core` als WASM-Plugin: Recall, Quiz, Guess, Score-Hook; Migration 4; `.locora` v2 nur bei Quiz/Guess | erledigt (API 0.4) |
| 4 | `locora plugin install/remove/ls` mit Rechteanzeige, Zustimmung, geschützten eingebauten Plugins | erledigt |
| 5 | `locora.toml` + `locora build` (`--check`, `--replace`) und `locora dump`; Doku `format/locora-toml.md`, Crate `locora-source` | erledigt |
| 6 | `locora export --with-progress` / `import [--without-progress]`: Review-Plan als `plugins/review/progress.json`, Dienst `review.restore@1` | erledigt |
| 7 | Review-Log im Plugin (Dienst `review.log@1`), `locora optimize` (FSRS-Gewichte nativ aus dem Verlauf, `review.configure@1`), `locora settings` (neue Loci pro Tag, Ziel-Behaltensrate), `locora stats`; Tage zählen in UTC | erledigt |
| 8 | Sammlungen (mehrere Paläste mit gemeinsamer Navigation, `navigate`-Aktion) | erst wenn gebraucht |

**Phase 1 ist damit abgeschlossen** (Punkt 8 bleibt bewusst offen).

Die Installationszustimmung für fremde Plugins bindet inzwischen Manifest und SHA-256 der
installierten WASM-Datei. Der Host prüft die Bytes vor dem Kompilieren und das Manifest vor der
Aktivierung. Alte Belege ohne Hash werden nicht mehr akzeptiert; betroffene Plugins müssen entfernt
und erneut installiert werden. Ed25519-Signaturen und Plugin-Updates bleiben offen.

**Tutorial als Palast (erledigt, Teil von Phase 4 vorgezogen):** Das Tutorial besteht aus Loci mit `.sply`-Grafiken:
`assets/tutorial/locora-lernen.locora.toml` (3 Räume, 9 Orte, Recall/Quiz/Guess), Grafiken per
`assets/tutorial/src/tutorial.py`, Simo und Glühwürmchen aus `assets/mascot/`. In die CLI eingebaut: `locora tutorial
[--reset]`. Dafür neu: Medientyp `application/vnd.splinely` (`.sply`) für Raum-Hintergründe und Bilder an Orten,
Locus-Bilder in Store (Migration 5), `.locora` (Version 2) und `locora.toml` (`image`), `locora add/edit --image`.
Bild-Hashes im Palast-Dienst `core.palaces@1` sind umgesetzt. Das offizielle `tutorial`-Plugin bietet
eine erste App-Tour über den berechtigungsgeprüften Host-Dienst `ui.highlight@1`: drei Schritte auf
der Startseite mit Simo, Weiter/Schließen und gespeichertem Schrittstand. Tour-Schritte für Palast,
Editor und Journey folgen.

Der Formel-Guide „Formeln verstehen“ ist als eigener Palast mit sechs `.sply`-Orten umgesetzt.
Eingaben, `Integer`, `Major` und Bindungen werden mit echten Workbook-Formeln gezeigt;
`locora formula-guide [--reset]` und die Startseite legen ihn an.

Das offizielle `gamification`-Plugin leitet XP, Lerntage, Wochenziel, Trophäen und
Palastmedaillen aus Fakten ab, die jedes Gerät unter eigenen Schlüsseln speichert (4.7); der
native Lernbeginn bestimmt den Lerntag. `locora game`, `locora stats`, die App-Kachel, die
Fortschrittsleiste der Journey und die Erfolge zeigen den Stand.

**Antworten und Abfragen (erledigt, aus dem Abgleich mit mnemodim):** Quiz mit bis zu 30 Optionen, pro Frage alle
richtigen und drei zufällige falsche, gemischt (`Answer::quiz_selection`); Guess vergleicht ohne Groß-/Kleinschreibung,
Satzzeichen und doppelte Leerzeichen (`normalize`) und nimmt weitere Antworten an (`accept`, `--accept`); Merkhilfe
`note` pro Locus (Migration 7, `.locora` v2, `locora.toml`, `--note`), nach dem Aufdecken gezeigt; Übungsarten
`--reverse` (Inhalt → Ort), `--numbers` („Ort 7 · Raum“) und `--shuffle`. Plugin-API inzwischen 0.6.

Die App spielt `.sply`-Bilder der Orte mit der Splinely-Runtime ab. Die nächsten
Schritte stehen in [`OFFEN.md`](OFFEN.md).

Behoben: Beim palastübergreifenden Review bekam jeder Palast die gesamte Laufzeit gemeldet (`lernzeit` zählte doppelt);
jetzt zählt nur die Zeit an den eigenen Loci. Die CLI-Tests waren gelegentlich rot, wenn ein Prozess vor dem Lesen der
Testeingabe endete (Broken Pipe); die Test-Helfer behandeln das jetzt als normal.

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

**Stand 16.10.2026 – erstes Gerüst läuft** (`apps/app/`, Start: `cd apps/app && cargo tauri dev`, siehe
`apps/app/README.md`): Startbildschirm mit „Weiter“-Zeile, Palastkarten mit ▶ und Tutorial-Knopf; Palastansicht mit
Räumen als Tabs, Hintergrund und `.sply`-Markern (Zustand neu/fällig/gewusst über die Runtime), Klick auf einen Marker
zeigt den Ort, Aufdecken zeigt Inhalt, `.sply`-Bild und Merkhilfe; Journey mit Recall (Leertaste, 1–4), Guess, Quiz und
den Übungsarten rückwärts/Ortsnummern/gemischt. Gemeinsam mit der CLI: `locora-runtime` (Plan, Fälligkeit,
Tageslimit, Tutorial, Bewertung über das Core-Plugin).

**Was in der App noch fehlt** (Reihenfolge = Empfehlung):

1. **Bild beim Aufdecken auch am Ort — umgesetzt:** `.sply`/Bild erscheint nach dem
   Aufdecken zusätzlich direkt am Locus auf der Bühne, in Palastansicht und Journey; vorher nur der Marker.
   `image_size` ist der Anteil der Raumbreite (Standard 0,25, endlicher Wert von 0,05 bis 1 einschließlich).
   Store-Migration 8, `.locora` (`imageSize`, Format 3 bei eigener Größe, sonst weiterhin Format 1/2),
   `locora.toml` und CLI `add`/`edit --image-size` sind umgesetzt. Bestehende Daten erhalten 0,25.
   Rückwärts bleibt das Bild Teil der Frage, nicht des aufgedeckten Ortsbilds. Regressionstests decken Migration,
   Validierung, Roundtrips, Formatkompatibilität, CLI und Aufdeckzustände ab; Workspace-/App-Tests, Clippy,
   Formatprüfung und Wasm-Check bestanden. Visuelle WebView-/E2E-Prüfung steht noch aus.
   Ziehgriff und Zahlenfeld für die Bildgröße sind jetzt im Editor vorhanden (Punkt 2).
2. **Editor — erster schreibender Schnitt umgesetzt:** „Bearbeiten“ in der Raumansicht: Klick auf die Bühne
   setzt einen Locus, Marker lassen sich ziehen oder mit Pfeiltasten verschieben. Ort, Inhalt, Merkhilfe,
   Recall/Guess (`accept`)/Quiz, gespeicherte Bildreferenz und Bildgröße sind inline bearbeitbar; gültige
   Inhalte speichern automatisch, ohne Speichern-Knopf. Ungültige Zwischenstände bleiben als Entwurf sichtbar;
   Raumwechsel/Journey sind bis zur Speicherung oder zum Verwerfen gesperrt. Löschen mit Bestätigung.
   Tauri prüft Palast-/Raumzugehörigkeit, Store validiert alle Mutationen; getrennte Geometrie-Patches
   erhalten Antworten und Workbook-Bindings. Nach einem erfolgreichen Write ist die Rückgabe unabhängig
   von einem weiteren Plugin-Aufruf. Tests für CRUD, Validierung, Persistenz, Geometrie und Eingabeprüfung.
   **Nächster Schnitt umgesetzt (6.3):** Liste auf der fokussierten Bühne mit Strg/⌘+V oder im expliziten
   Textfeld mit Knopf als Route hinzufügen:
   je nichtleerer Zeile ein Recall-Locus, 1–100 Begriffe, maximal 500 UTF-8-Bytes pro Begriff und 64 KiB
   Eingabetext. Neue Orte werden in Listenreihenfolge als Schlangenroute angehängt; bestehende bleiben
   unverändert. Tauri validiert Zugehörigkeit und sämtliche Begriffe, der Store speichert Loci und Op-Log
   in einer Transaktion (vollständiger Rollback bei Fehlern). Video-Räume bleiben ausgeschlossen.
   Tests decken Parsing, Routengeometrie, Grenzen, Scope, Persistenz und Rollback bei SQL-Fehlern ab.
   WebView-/E2E-Prüfung und das Bühnen-Strg+V-Klickbudget sind noch nicht nachgewiesen;
   normales Einfügen in anderen Eingabefeldern wird nicht abgefangen.
   **Mediendatei-Auswahl umgesetzt:** „Bild wählen…“ öffnet den nativen Tauri-Dialog für den ausgewählten
   Locus; „Bild entfernen“ löscht die Zuordnung. PNG/JPEG/WebP/GIF/AVIF werden nach Endung und
   MIME-Signatur geprüft, `.sply` zusätzlich durch den vorhandenen SVG-Konverter (maximal 64 MiB).
   Exakte Palast-/Raum-/Ort-Zugehörigkeit wird vor Auswahl und Import geprüft; Abbruch schreibt nichts,
   Fehler ändern die Bildzuordnung nicht. Keine Pfad-IPC, keine neuen WebView-Dateirechte; CSP unverändert.
   Backend-Tests für Import, Persistenz, MIME-/Formatfehler und Scope; native WebView-Prüfung noch offen.
   **Startknopf umgesetzt:** „Bild/Video wählen…“ öffnet einen nativen Dialog ohne Pfad-IPC oder
   zusätzliche WebView-Dateirechte. Gemeinsame Prüfung für Rasterbilder, binäre `.sply` und
   MP4/M4V/WebM/MOV-Signaturen; nur reguläre lokale Dateien, maximal 64 MiB.
   Dateiname wird Palast-/Raumname (Fallback „Neuer Palast“); belegte Palastnamen erhalten
   „ (2)“, „ (3)“ usw. (NOCASE wie im Store). Palast, passender Bild-/Video-Raum,
   Medienmetadaten und Op-Log werden atomar gespeichert. Abbruch schreibt nichts; Fehler hinterlassen
   keine leeren Paläste oder partiellen DB-Änderungen (unreferenzierte Hash-Dateien können bleiben).
   Erfolg öffnet direkt den Editor; Zurück lädt die Startseite neu. Tests für Signaturen, Grenzen,
   Abbruch, Persistenz, Namen und DB-Rollback; nativer WebView-Smoke-Test noch offen.
   **Umgesetzt:** Datei hineinziehen über einmalige Tickets (Pfad bleibt im Backend; Startseite → neuer Palast,
   Editor → Ortsbild); Undo/Redo (Inverse für Inhalt/Bild/Position/Größe, Backend-Belege `editor_receipts` für
   Hinzufügen/Einfügen/Löschen mit Konfliktprüfung, neueste 500); jeder Formularstand liegt in `localStorage` und
   wird nach Absturz zum Wiederherstellen/Verwerfen angeboten.
   **Noch offen:** einzelne ungebundene Felder an Workbook-Orten bearbeiten. Browser-/WebView-Gesten, Drop,
   Undo-Tastatur, Fokus bei Autosave und Klick-Budgets manuell/E2E prüfen (`docs/app-editor-testplan.md`,
   `apps/app/tests/native_smoke.py`).
3. **Workbook in der App — umgesetzt:** Eingaben (`Inputs`) werden vor der Journey abgefragt (vorbelegt mit dem
   Standardwert, gesetzt vor der Planung, damit `active`-Formeln sie sehen); Aktionen (`Actions`) erscheinen als Knöpfe
   unter der Frage, `reveal`-Aktionen decken auf (Guess/Quiz zählen dann als aufgegeben). Formelfehler erscheinen als
   Text im Feld. Eingabefelder passen zum Typ (Zahlenfeld, Ja/Nein-Kästchen). Aktionen gehen auch nach dem Bewerten (ändern nur die angezeigte Antwort, nicht die Bewertung).
4. **Startbildschirm vollständig:** Einstellungen (neue Loci pro Tag, Behaltensrate) und Statistik sind umgesetzt
   (⚙, gleiche Berechnung wie `locora settings`/`stats` über `locora_runtime::settings`). Import/Export von
   `.locora` (Dateidialoge, mit Wiederholungsstand; Logik in `locora_runtime::transfer`, gemeinsam mit der CLI) ist
   umgesetzt. Plugin-Kacheln (`ui.tile`) erscheinen auf dem Startbildschirm; die letzte Kachel je Plugin speichert das Profil
   (`Profile::tiles`). Die Befehlspalette (Strg/⌘+K) findet Paläste und Befehle per Tastatur.
   **`locora.toml`-Import umgesetzt:** Der Importdialog nimmt auch TOML-Quellen an, prüft sie mit
   `locora-source` und löst Medien relativ zur Quelldatei auf. Import erzeugt neue Kopien mit freien
   Namen statt bestehende Paläste zu ersetzen; TOML enthält keinen Wiederholungsstand.
   Backend-Tests für Medien, Namenskonflikte, Neustart und ungültige Quellen; nativer Dialog-Smoke-Test offen.
   **TOML-Export umgesetzt:** „TOML exportieren…“ wählt einen lokalen Zielordner und schreibt
   Quelle und Medien mit `locora-source::dump`, ohne bestehende Dateien zu überschreiben.
   Kein Wiederholungsstand im TOML; Roundtrip und Überschreibschutz sind backend-getestet.
   Nativer Ordnerdialog und Abbruch bleiben manuell zu prüfen.
5. **Grafiken als `.sply`** (Stil `assets/STIL.md`): umgesetzt sind Kachel-Icons (core, review, notify, tutorial,
   gamification, andere Plugins) und Journey-Feedback (richtig/nochmal/geschafft) aus `assets/ui/src/ui.py` sowie der
   Willkommensbildschirm mit Simo (`assets/mascot/src/simo.py`). Ebenfalls umgesetzt: Raum-Arten (Tabs), „nichts fällig“
   (Simo), Ladezustand (Logo).
6. **Leistung:** Alle Marker eines Raums laufen in einem gemeinsamen SVG-Frame mit einer Runtime
   (`/markers/<hash>`, `marker_layer.js`; IDs je Klon umbenannt, Zustände per Snapshot). Prüfskript für WebKitGTK:
   `apps/app/tests/marker_layer_probe.py`. **Medien mit Range-Anfragen umgesetzt:** Einzelbereiche,
   offene und Suffix-Bereiche liefern 206 mit `Content-Range`; nicht erfüllbare Bereiche 416.
   Es wird nur der angeforderte Dateiausschnitt gelesen; HEAD liefert Metadaten ohne Body.
   Multipart/fehlerhafte Bereiche fallen auf die vollständige Antwort zurück. Backend-getestet;
   Video-Spulen in der nativen WebView noch manuell prüfen.
7. **Video-Räume** (Phase 3): am Locus anhalten, Zeitleiste.
   Die App-Zeitleiste hat nun einen freien Suchregler; ein Bühnenklick kann einen zeitgebundenen Ort
   an der Bildposition setzen. Marker erscheinen ab einem einstellbaren Vorlauf vor ihrer Locus-Zeit;
   der Vorlauf kann je Ort im Editor überschrieben werden. Video- und animierte `.sply`-Orte wurden
   nach freiem Spulen per Bühnenklick in der nativen WebView platziert. Das `.sply`-Markerfenster
   wurde dort vor und am Locus-Zeitpunkt geprüft; Video-Markerfenster und pro-Ort-Vorlauf bleiben offen.
   **Erster Editor-Schnitt umgesetzt:** Video-Orte über eine nichtnegative Sekundenangabe anlegen,
   inkl. Backend-Validierung (endlich, Video-Raum) und Persistenztest. Normales Ortsetzen bleibt
   in Video-Räumen deaktiviert. **Wiedergabezeit übernehmen umgesetzt:** „Ort an aktueller Videozeit“
   pausiert das geladene Video und legt einen Ort am Playhead an. Taste L wirkt nur auf der
   fokussierten Bühne (keine Texteingaben, Videosteuerung oder Marker; kein Key-Repeat).
   **Erste Zeitleisten-Navigation umgesetzt:** chronologische, horizontal scrollbar angeordnete
   Ortsknöpfe mit Sekundenangabe und Routennummer; Klick pausiert, springt zum Zeitpunkt und
   wählt den Ort aus. Zeitwerte werden im gemeinsamen `LocusView` projiziert und getestet.
   **Erster Journey-Schnitt umgesetzt:** Video-Fragen erhalten die Locus-Zeit im gemeinsamen IPC-Vertrag;
   das Video läuft stumm bis zum Locus und hält dort für die Frage an. Beim nächsten vorwärtsliegenden
   Locus desselben Videos startet es am zuletzt erreichten Locus; bei Medienwechsel/Rückwärtsrichtung bei 0.
   Während der Wiedergabe sind Antworten und Workbook-Aktionen gesperrt (auch per Tastatur).
   Ungültige Zeiten/Ladefehler werden angezeigt und geben die Antwort frei. Bei verweigertem Autoplay
   wird mit Hinweis direkt zum Locus gesprungen. Nach Bewertung werden manuelle Videokontrollen freigegeben.
   Die Editor-Zeitleiste markiert Ortszeiten proportional zur geladenen Videodauer; Orte nach dem
   Videoende werden gekennzeichnet und nicht angesprungen. Orte lassen sich per Zeitregler verschieben;
   der Backend-Befehl prüft Raumzugehörigkeit und endliche, nichtnegative Zeiten. Beim ersten Ort bei
   0 Sekunden zeigt die Journey eine Vorschau aller dort platzierten Orte. Die Zeitgrenze nutzt
   `requestVideoFrameCallback`, wenn die WebView es anbietet; `timeupdate` und ein 16-ms-Timer
   bleiben als Fallback. Der Timer endet am Ziel oder bei Ladefehlern. Das UI-Release-Bundle baut;
   die native Sichtprüfung dieser Funktionen und gedrosselter WebViews steht noch aus.
8. **Mobil und CI:** Android-/iOS-Builds (`cargo tauri android|ios init`), Linux-Pakete in CI, E2E-Tests der
   Klick-Budgets (6.3), Barrierefreiheit (Tastatur, Screenreader-Beschriftungen) prüfen.
9. **Später:** Ton beim Aufdecken über Audio im `.sply`.

### 13.5 Danach (Phasen 3–7)

- **Phase 3:** Video-Räume in der App (anhalten am Locus bis gelöst, Editor mit Taste L, Zeitleiste), Plugin `notify`
  (4.6: lokale Benachrichtigungen, Ruhezeiten, Bündelung).
- **Phase 4:** Plugin `tutorial`: der Tutorial-Palast existiert schon (`locora tutorial`); dazu kommen Touren durch die
  App über `ui.highlight@1`, ein Formel-Guide (ebenfalls als Palast mit `.sply`-Bildern) und Simo als Begleiter.
- **Phase 5:** Plugin `gamification` (lokal und kostenlos; seit 0.2.0 die Regeln aus 4.7), danach privates Repo `locora-cloud` (axum + Postgres, Abo, Ranglisten,
  Ed25519-Entitlements mit 14 Tagen Offline-Kulanz). Bezahlt wird später nur im Web, nicht per In-App-Kauf (Abschnitt 10).
- **Phase 6:** Sync über das Op-Log (HLC, Last-Writer-Wins pro Feld) mit Locora Cloud.
- **Phase 7:** Plugin-Verzeichnis, Signaturen, WASM-Plugins für Dritte inkl. deklarativer UI (4.4) und Store-Prüfung.

**Stand 18.10.2026:** `.locora-plugin`-ZIP-Pakete werden mit `locora plugin pack` erstellt und
bei Prüfung, Installation und Update begrenzt und gegen das eingebettete WASM-Manifest geprüft.
Der manuelle Cloud-Sync sowie Anmeldung, Registrierung und Schlüsselaufbau sind in den App-Einstellungen
erreichbar; der vollständige Durchlauf gegen einen realen Server und auf Mobilgeräten steht noch aus.
Neue Tutorial- und Formel-Guide-Paläste verwenden feste Kennungen über
Geräte hinweg. Für vorhandene Paläste ist ein bewusster Neuaufbau nötig, der den bisherigen
Wiederholungsstand zurücksetzt. Der Gamification-Fortschritt wird seit Plugin 0.2.0 verlustfrei
zusammengeführt (Fakten pro Gerät, 4.7); ältere App-Versionen zeigen bis zum Update ihre alten Zahlen.
Die Vorlagen `befehl`, `locus-typ` und `importer` sind als aufrufbare Dienste angelegt;
die automatische Journey-Einbindung eines Locus-Typs und `plugin dev --device` benötigen noch
zusätzliche Host- und App-Schnittstellen. Zeitgebundene `.sply`-Räume nutzen die Splinely-Runtime
in Editor und Journey; steuerbare GIF-Räume bleiben offen.
Die App kann auf diesem Gerät direkt in die Core-Palastübersicht starten. Das Vorschaubild
einer Palastkarte lässt sich unter den Räumen mit Bild oder `.sply` wechseln; Video-Räume
zeigen ein Raum-Icon, damit die Übersicht nicht jedes Video vollständig lädt. Locora Cloud
begrenzt neue Medienuploads pro Konto mit einem atomaren Stundenzähler; Kontenexport und
-löschung benötigen weiterhin einen Ablauf für S3-Medien und Billing-Ereignisse.
Core-Start und Raumvorschau wurden im nativen WebKitGTK über App-Neustarts geprüft; ein
explizites `LOCORA_DATA`-Profil kapselt dafür auch den WebKit-Speicher.
Angemeldete Nutzer können ihr Cloud-Passwort per API und `locora cloud password` ändern; der Server widerruft danach
alle Sitzungen. Ein Reset ohne bestehende Anmeldung benötigt noch einen Mail- und Token-Ablauf.
