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
| **Alles ist ein Plugin** | `core`, `tutorial` und `gamification` sind Plugins mit derselben API. Der Host selbst kann nichts außer Plugins laden. |
| **Open Core** | Host, SDK, `core`, `tutorial` und CLI sind Open Source. `gamification` (Web-Service, z. B. 1 €/Monat) ist proprietär. |
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

## 3. Repository-Struktur

```text
locora/                          (öffentlich, MIT)
├── crates/
│   ├── locora-sdk/              Plugin-API: Traits, Events, Services, UI-Beiträge, WIT-Definition
│   ├── locora-host/             Plugin-Loader, Event-Bus, Service-Registry, Berechtigungen
│   ├── locora-store/            SQLite (rusqlite), Migrationen, Operations-Log, Medien-Store
│   ├── locora-format/           .locora v2 lesen/schreiben, Import von .mnemodim v1
│   ├── locora-formula/          Port von shared/formulas.ts + workbookRuntime.ts
│   └── locora-sync/             (später) Op-Log-Sync-Client
├── plugins/
│   ├── core/                    Paläste, Stages, Loci, Video-Stages, Journeys, Workbook, Wiederholung
│   └── tutorial/                Onboarding, interaktive Touren, Formel-Guide
├── apps/
│   ├── cli/                     `locora` Binary (clap)
│   └── app/                     Tauri 2 (src-tauri/) + Leptos-UI (ui/)
├── format/                      Spezifikation .locora v2 + Beispiel-Dateien + JSON-Schema
└── docs/

locora-gamification/             (privat, proprietär)
├── plugin/                      Client-Plugin: XP, Herzen, Combos, Trophäen, Daily Challenges, Leaderboards
└── server/                      axum + Postgres: Abo, Leaderboards, Sync der Statistiken
```

Offizielle Builds binden `locora-gamification` per Cargo-Feature ein (`--features official`).
Der Open-Source-Build läuft komplett ohne das Plugin.

---

## 4. Plugin-Architektur

### 4.1 Grundprinzip

Der Host kann nur drei Dinge: **Plugins laden**, **Nachrichten vermitteln** und **UI-Beiträge anzeigen**.
Alles Fachliche steckt in Plugins, auch der Kern.

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;      // id, version, requires, optional, permissions, license
    fn activate(&self, ctx: &mut PluginContext) -> Result<()>;
    fn deactivate(&self, ctx: &mut PluginContext) -> Result<()> { Ok(()) }
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
Dadurch funktioniert dieselbe API später auch über die WASM-Grenze.

### 4.2 So reden die drei Plugins miteinander

```text
core ──event: journey.locus_answered──▶ gamification (Combo, Herz-Verlust)
core ──hook:  journey.score ──────────▶ gamification (Multiplikator) ──▶ zurück an core
core ──event: palace.created ─────────▶ tutorial (nächster Tour-Schritt)
tutorial ──service: core.palaces@1 ───▶ core (Beispielpalast anlegen)
tutorial ──service: ui.highlight@1 ───▶ host (Element hervorheben)
gamification ──event: trophy.unlocked ▶ tutorial (erklärt Trophäen beim ersten Mal)
```

Alle Abhängigkeiten außer `core` sind **optional**. Fehlt Gamification, rechnet `core` die Journey
einfach ohne Hook. Fehlt Tutorial, passiert gar nichts.

### 4.3 Open Core: was ist offen, was kostet

| Teil | Lizenz | Preis |
|---|---|---|
| Host, SDK, Store, Format, Formula, CLI | MIT | frei |
| Plugin `core` | MIT | frei, voller Funktionsumfang offline |
| Plugin `tutorial` | MIT | frei |
| Plugin `gamification` | proprietär | Web-Service im Abo (Leaderboards, Cloud-Challenges, Statistik-Sync) |

Das SDK steht bewusst unter einer permissiven Lizenz und nicht unter GPL. So dürfen proprietäre
Plugins (deins und später die von Dritten) gegen das SDK linken. Für externe Beiträge in den
offenen Repos sollte man ein DCO/CLA einführen, damit du die Lizenz später noch ändern kannst.

**Freischaltung offline:** Der Server stellt ein Ed25519-signiertes Entitlement-Token aus
(`plugin`, `plan`, `expires`). Die App prüft es lokal mit dem eingebauten Public Key und gewährt
14 Tage Kulanz ohne Netz. Es gibt kein „Phone home“ bei jedem Start.

### 4.4 Drittanbieter-Plugins (Phase 7)

- Gleiche API als **WIT-Interface** (WebAssembly Component Model).
- Desktop und Android laufen mit **wasmtime**. iOS verbietet JIT, deshalb dort wasmtime mit dem Interpreter **Pulley** (oder `wasmi`).
- Berechtigungen werden im Manifest deklariert (`storage`, `palaces:read`, `palaces:write`, `network`) und beim Installieren bestätigt.
- Eigene UI von Drittanbietern läuft in einem sandboxed iframe mit `postMessage`-Brücke. Die eingebauten Plugins nutzen native Komponenten.

---

## 5. Datenmodell und Offline first

- **SQLite** pro Profil (`~/.local/share/locora/` bzw. App-Sandbox auf Mobile), WAL-Modus.
- **Operations-Log:** Jede Änderung wird als Op mit Hybrid-Logical-Clock-Zeitstempel gespeichert
  (Last-Writer-Wins pro Feld, Tombstones für Löschungen). Das reicht für den späteren Sync,
  ohne dass jetzt schon ein Server existieren muss.
- **Medien:** Sie werden nach SHA-256 benannt (`media/ab/cd…`). Das liefert automatisches Deduplizieren, einfachen Sync und unveränderliche Dateien.
- **Journeys** werden vollständig lokal bewertet. Eine Server-Validierung (wie bisher in Convex) gibt es nur
  für Gamification-Ranglisten. Dort wird das Transkript wie heute serverseitig nachgespielt, mit derselben Rust-Crate kompiliert für den Server.
- **Wiederholung:** In `core` kommt FSRS (`fsrs` Crate) dazu und plant fällige Loci.
  Damit kann der Startbildschirm direkt „7 Loci fällig“ zeigen. Das bisherige *Mastery* wird darauf abgebildet.

### Was aus mnemodim wohin wandert

| mnemodim heute | Locora |
|---|---|
| Paläste, Stages, Loci, Recall/Quiz/Guess | `core` |
| Workbook, Formeln, Major-Tabellen, Actions, Collections | `core` (`locora-formula`) |
| Import/Export `.mnemodim` | `locora-format` (v1 lesen, v2 schreiben) |
| Timing-Punkte, Combos, Herzen, XP, Trophäen, Daily Challenges | `gamification` |
| Formel-Guide, Onboarding | `tutorial` |
| Auth, Convex | entfällt lokal; Account nur für den Gamification-Service |
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
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐        │
│  │  Core    │  │ Tutorial │  │ Gamification │        │
│  │ 12 Pal.  │  │ 3/8 ✓    │  │ 🔥 5 Tage    │        │
│  └──────────┘  └──────────┘  └──────────────┘        │
│  + Plugin hinzufügen                                 │
└──────────────────────────────────────────────────────┘
```

- Jede Kachel ist ein **Live-Widget** des Plugins (deklarativer UI-Beitrag), nicht nur ein Icon.
- Die **Weiter-Zeile** ist die wichtigste Aktion. Sie kommt aus `core` (fällige Wiederholung oder zuletzt bearbeitet).
- **Befehlspalette** (Strg/⌘+K, auf Mobile Wischen nach unten) durchsucht alle Befehle aller Plugins und alle Paläste.
- Einstellung „Direkt in Core starten“ für alle, die den Übersichtsschritt nicht wollen.

### 6.2 Core-Ansicht

Links bzw. oben steht die Palastliste, jede Karte mit **▶ (Journey starten)** direkt auf der Karte.
Es gibt keine Zwischenseite „Palast-Details“: Ein Klick auf die Karte öffnet sofort den Editor.

### 6.3 Klick-Budgets (verbindlich, werden als E2E-Tests geprüft)

| Aufgabe | mnemodim heute | Locora-Ziel |
|---|---|---|
| Journey für einen Palast starten | 3–4 | **1** (▶ auf Karte oder Enter auf „Weiter“) |
| Neuen Palast mit Bild anlegen | ~6 + Formulare | **1** (Bild/Video ins Fenster ziehen = Palast + Stage + Editor offen) |
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

Eine Stage hat `kind: "image" | "video"`. Ein Video-Locus enthält zusätzlich:

```json
{
  "id": "l7",
  "label": "Merkur",
  "memory": "kleinster Planet",
  "at": 83.40,              // Sekunden; 0 = „am Anfang“
  "pos": [0.42, 0.61],      // optional: Marker auf dem Videobild
  "pause": "until_solved",  // "until_solved" | "brief" (3 s) | "none" (nur Marker)
  "answer": { "type": "recall" }   // recall | quiz | guess, wie Bild-Loci
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

# Aus Datei (eine Zeile = ein Locus, optional "Begriff | Erinnerung | x,y")
locora new "Vokabeln" --image buero.jpg --from vokabeln.txt

# Video-Palast mit Zeitmarken
locora video "Vortrag" vortrag.mp4 --loci 0:12=Einleitung 1:23=These 4:05=Fazit

# Bearbeiten
locora add Planeten "Saturn" --memory "Ringe" --at 0.7,0.3
locora ls                      # Paläste mit fälligen Loci
locora practice Planeten       # Abfrage im Terminal (TUI, Text-Recall)
locora export Planeten -o planeten.locora
locora import alt.mnemodim     # Altbestand übernehmen

# Plugins
locora plugin ls
locora stats                   # kommt aus dem Gamification-Plugin, falls installiert
```

Zusätzlich gibt es ein menschenlesbares **Quelltext-Format** (`*.locora.toml`), das man in Git versionieren und mit
`locora build palast.locora.toml` zu einem Palast machen kann. Damit lassen sich auch Paläste mit KI oder Skripten erzeugen.

---

## 9. Dateiformat `.locora` (v2)

Der Container bleibt ZIP. Die Inhalte werden vereinfacht:

| v1 (`.mnemodim`) | v2 (`.locora`) | Warum |
|---|---|---|
| CSV mit JSON-Zellen | **JSON pro Palast** (`palaces/<id>.json`); CSV nur noch für Tabellen | einfacher zu erzeugen und zu validieren, keine doppelte Kodierung |
| `assets/<id>` mit eigener ID | `media/<sha256>.<ext>` | dedupliziert, sync-freundlich, Integrität eingebaut |
| nur Bild/Audio | + **Video** (MP4) | Video-Paläste |
| fester Inhalt | **`plugins/<plugin-id>/…`** Namensräume | Plugins speichern eigene Daten; fehlt ein Plugin, bleiben seine Daten unverändert erhalten |
| keine Abhängigkeiten | `manifest.requires: [{ plugin, version, required }]` | App kann sagen „Benötigt Plugin X“ statt still zu scheitern |
| — | JSON-Schema in `format/` | Fremdtools und KI können valide Dateien bauen |

Beibehalten werden: harte Limits, „fail closed“ bei Unbekanntem, keine Formel-Ausführung beim Import und Import als neue Kopie.
Die Limits werden an Video angepasst (z. B. 2 GiB gesamt, ZIP64, Video-Einträge *stored* statt deflate).

---

## 10. Gamification und Bezahlung

- **Lokal kostenlos?** Offene Frage (Abschnitt 12). Vorschlag: XP, Combos und Trophäen laufen auch ohne Abo **lokal**.
  Das Abo zahlt man für den **Web-Service**: Leaderboards, gemeinsame Daily Challenges, Freunde, Statistik-Sync über Geräte.
  So bleibt das Abo fair, und die Offline-Erfahrung ist vollständig.
- **App-Store-Regeln:** Auf iOS und Android müssen digitale Abos über In-App-Kauf laufen (Apple/Google nehmen 15 % im Small-Business-Programm).
  Auf Desktop und Web eignet sich ein Merchant of Record wie Paddle oder Lemon Squeezy (übernimmt EU-USt).
- **Preis 1 €/Monat:** Feste Gebühren pro Transaktion (z. B. ca. 0,30–0,50 €) fressen bei 1 € 30–50 %.
  Besser sind **9,99 €/Jahr** oder 1,99 €/Monat bzw. 14,99 €/Jahr.
- **Server:** Rust (axum + Postgres) im privaten Repo. Er nutzt dieselben Crates (`locora-formula`, Journey-Replay) für fälschungssichere Ranglisten.

---

## 11. Roadmap

| Phase | Inhalt | Ergebnis |
|---|---|---|
| **0** | Repo, Cargo-Workspace, CI (Linux/macOS/Windows), Lizenzen, Entscheidungen aus Abschnitt 12 | leeres, baubares Gerüst |
| **1** | `locora-store`, `locora-formula` (Port inkl. der bestehenden Testfälle aus `formulas.spec.ts` etc.), `locora-format` (v2 + v1-Import), Plugin-Host + SDK, `core` ohne UI, **CLI** | Paläste per CLI anlegen, üben, importieren und exportieren |
| **2** | Tauri-App: Startbildschirm, Befehlspalette, Core-Editor, Journey (Bild). **Android- und iOS-Builds ab Tag 1 in CI** | nutzbare App auf allen Plattformen |
| **3** | Video-Stages: Editor + Journey + CLI | Video-Paläste |
| **4** | Plugin `tutorial` (Touren über `ui.highlight`, Beispielpalast, Formel-Guide) | Onboarding |
| **5** | Plugin `gamification` lokal + Server + Bezahlung + Entitlements | erstes Einkommen |
| **6** | Sync (Op-Log ↔ Server) als optionaler Dienst | mehrere Geräte |
| **7** | WASM-Plugins für Dritte | offenes Ökosystem |

Die Klick-Budgets aus 6.3 und die Startzeit werden ab Phase 2 als Tests in CI geprüft.

---

## 12. Entscheidungen

| Thema | Entscheidung |
|---|---|
| Name | **Locora** |
| Sprache | **Pures Rust**, auch die UI (Tauri 2 + Leptos) |
| Lizenz der offenen Teile | **MIT** |
| Repository | `wasser-quest/locora` |

### Noch offen

1. **Gamification ohne Abo:** Laufen XP und Trophäen lokal gratis, und kostet nur der Web-Service (Leaderboards, Sync, gemeinsame Challenges)?
2. **Preis:** 1 €/Monat oder ein Jahresabo (wegen der Transaktionsgebühren)?
