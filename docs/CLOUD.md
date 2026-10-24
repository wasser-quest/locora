# Locora Cloud aus Sicht der App

Locora läuft ohne Konto und ohne Netz. Locora Cloud ist ein optionaler Dienst, der deine Daten
zwischen deinen Geräten abgleicht (Desktop, Android, iPhone/iPad, Browser und CLI). Der Server
ist proprietär und liegt im privaten Repository `wasser-quest/locora-cloud`; dort stehen API,
Grenzen und Betrieb (`README.md`, `deploy/BETRIEB.md`). Diese Seite beschreibt, was die App tut.

## Was synchronisiert wird

Alles, was im Op-Log landet: Paläste, Räume und Orte, Tabellen und Formel-Bindungen, die Daten
der Plugins (z. B. Wiederholungsplan und Gamification-Stand) und Journeys. Medien (Bilder,
Videos, Ton, `.sply`) gehen einzeln nach ihrem Hash und werden nur einmal hochgeladen.

Konflikte löst jedes Feld für sich: die spätere Änderung gewinnt (HLC, Last-Writer-Wins pro Feld).
Gelöschtes bleibt als Grabstein (Tombstone) gelöscht, auch wenn ein anderes Gerät später noch
eine ältere Änderung schickt.

Der Spielfortschritt verliert dabei nichts: jedes Gerät schreibt eigene Fakten unter eigenen
Schlüsseln und ergänzt sie nur, die Summen leitet jedes Gerät daraus ab (siehe
[`PLAN.md`](PLAN.md), 4.7). Im Browser liegt er dafür im Profil des Kerns, nicht in IndexedDB. Den
alten Stand aus IndexedDB (nie synchronisiert) übernimmt der Browser einmal als eigenen Fakt
(`legacy/<gerät>/own`), der zum geteilten Stand des Desktops hinzukommt, statt ihn zu ersetzen.

## Spielfortschritt löschen

- „Spielfortschritt löschen“ in den Einstellungen (CLI: `locora game reset`) schreibt eine neue
  **Epoche** (`host/game-epoch`, ein Schlüssel, die letzte Löschung gewinnt). Nur Fakten der
  aktuellen Epoche zählen. Jedes Gerät leert ältere, sobald es die neue sieht; was ein Gerät
  offline unter der alten Epoche gesammelt hat, zählt auch dann nicht, wenn es erst später ankommt.
  Löschen zwei Geräte gleichzeitig, gilt das spätere.
- Ist das Gerät angemeldet, bittet es den Server, die Spielpunkte des Kontos für die Ranglisten
  zu löschen: `DELETE /v1/game/data` mit der Sitzung (204: erledigt, auch wenn nichts da war).
  Ein älterer Server kennt das nicht (404/405): die Ranglisteneinträge bleiben, bis er
  aktualisiert ist, und die App sagt das. Solange der Server nicht 204 geantwortet hat (älterer
  Server, Netz, 5xx, 401), merkt sich das Gerät die Bitte für dieses Konto
  (`game-rankings-pending.json` im Profil, nur lokal, getrennt von der Sitzung in `cloud.json`,
  die eine neue Anmeldung ersetzt) und stellt sie erneut: beim Start und bei jeder Statusabfrage
  der App, nach dem Anmelden (auch nach abgelaufener Sitzung) und vor jedem Sync, unabhängig vom
  Sync-Zugang (der Endpunkt braucht kein Abo). Nur das Gerät, das gelöscht hat, fragt, damit
  spätere Punkte nicht von einem nachzüglerischen Gerät gelöscht werden; ein anderes Konto auf
  dem Gerät erbt die Bitte nicht.
- Die älteren verschlüsselten Änderungen bleiben im Protokoll des Servers, bis du „Daten in der
  Cloud löschen“ wählst; lesen kann sie dort niemand. Danach sendet das Gerät seinen Stand neu,
  mit der neuen Epoche: ein neues Gerät beginnt bei 0.

## Drei Geheimnisse

| | Wofür | Wer kennt es |
|---|---|---|
| **Cloud-Passwort** | Anmelden bei Locora Cloud (10–256 Zeichen) | der Server prüft es (Argon2id) |
| **Verschlüsselungs-Passwort** | schließt den Kontoschlüssel auf, mit dem alles vor dem Senden verschlüsselt wird (mindestens 12 Zeichen) | nur du; der Server kennt es nicht |
| **Wiederherstellungsschlüssel** | setzt ein neues Verschlüsselungs-Passwort, wenn das alte vergessen ist | nur du |

- Die Daten sind Ende-zu-Ende-verschlüsselt: Änderungen und Medien werden auf dem Gerät
  versiegelt, der Server speichert nur undurchsichtige Blöcke. Er sieht E-Mail-Adresse,
  Anzeigename, Größen und Zeitpunkte, nicht die Inhalte. Nur Journeys für Ranglisten
  (CLI mit Abo) wertet er selbst und sieht dafür deren Ablauf.
- Der Wiederherstellungsschlüssel erscheint **nur einmal**, beim Einrichten der Verschlüsselung.
  Die App zeigt ihn in einem eigenen Feld mit „Als Datei speichern…“ und „Kopieren“; erst nach
  „Ich habe den Schlüssel gesichert“ geht es weiter. Ohne Verschlüsselungs-Passwort und
  Wiederherstellungsschlüssel kann niemand die Cloud-Daten wieder lesbar machen.
- Wiederherstellen geht bisher nur in der CLI: `locora cloud encrypt recover`.
- Ein vergessenes **Cloud-Passwort** setzt man per E-Mail zurück: „Passwort vergessen?“ im
  Anmeldeformular öffnet `https://web.locora.cloud/forgot`. Der Link in der Mail gilt eine Stunde.
  Das Zurücksetzen meldet alle Geräte ab und ändert nichts an der Verschlüsselung.
- Das Cloud-Passwort ändern: `locora cloud password` oder auf der Kontoseite.

## Konto und Testzeitraum

- Ein Konto wird in der App (Einstellungen → Locora Cloud → „Neues Konto erstellen“) oder mit
  `locora cloud register` angelegt.
- Jedes neue Konto ist ein Gastkonto mit einem Testzeitraum von **sechs Monaten** ab der
  Registrierung. In dieser Zeit synchronisiert die App ohne Abo. Nur ein bezahltes Abo bekommt
  eine signierte Freischaltung (Ed25519, 14 Tage Kulanz ohne Netz).
- Die Daten bleiben auf dem Server, solange du synchronisierst; ohne Synchronisation sechs Monate
  lang. Einen Monat vor dem Löschen kommt eine Warnung per Mail.
- Nach der Registrierung bittet die App, die E-Mail-Adresse zu bestätigen („Link erneut senden“,
  „Ich habe bestätigt – prüfen“). Der Link gilt 48 Stunden.
- Bezahlen geht nur im Web, nicht in den Apps (siehe unten).

## Synchronisieren

**Von Hand:** Einstellungen → Locora Cloud, Verschlüsselungs-Passwort eingeben,
„Jetzt synchronisieren“ (CLI: `locora cloud sync`).

- Vorher zeigt die App, was dieses Gerät sendet, nach Palast (neu, geändert, gelöscht), und wie
  viele Änderungen anderer Geräte in der Cloud warten. Nachher sagt sie, was von den anderen
  Geräten kam, mit Palastnamen.
- Bevor die App unnötige Arbeit sendet, fasst sie den Ausgang zusammen: Änderungen, die eine
  spätere Änderung vollständig ersetzt, bleiben weg (ein Wert, der wieder gesetzt wurde,
  Änderungen an einem Palast, der danach gelöscht wurde). Anlegen und Löschen werden immer
  gesendet.
- Große Mengen gehen in Paketen bis 1000 Änderungen und etwa 3,5 MB; Medien werden gesammelt
  geprüft und vier gleichzeitig übertragen.

**Auto-Sync:** „Auto-Sync einschalten“ fragt einmal das Verschlüsselungs-Passwort und behält den
aufgeschlossenen Schlüssel auf dem Gerät:

- Desktop und Mobil: im Schlüsselspeicher des Systems (Schlüsselbund auf macOS und iOS,
  Anmeldeinformationsverwaltung auf Windows, Secret Service unter Linux, Android Keystore).
  Gibt es keinen, ist Auto-Sync nicht möglich; der Schlüssel landet nie in einer einfachen Datei.
- Browser: in IndexedDB, eingepackt mit einem nicht exportierbaren WebCrypto-Schlüssel (AES-GCM).
- Die App synchronisiert beim Start und wenn sie wieder in den Vordergrund kommt, etwa 30 Sekunden
  nach der letzten Änderung und alle zehn Minuten. Immer nur ein Sync; nach Fehlern wartet sie
  länger, nach „zu viele Anfragen“ (429) mindestens 15 Minuten.
- Sie pausiert, wenn du handeln musst (neu anmelden, einmal von Hand synchronisieren), und zeigt
  „Zuletzt synchronisiert“.
- „Schlüssel auf diesem Gerät vergessen“ schaltet Auto-Sync aus. Abmelden, „Daten in der Cloud
  löschen“ und das Löschen des Kontos vergessen den Schlüssel ebenfalls.

## Einen Palast löschen

- Ein gelöschter Palast wird als Löschung (Grabstein) synchronisiert. Die App sendet sie erst,
  wenn du bestätigst: „Palast „X“ wird auch in der Cloud und auf deinen anderen Geräten
  gelöscht.“ Auto-Sync wartet auf diese Bestätigung und sendet bis dahin nichts.
- Auf den anderen Geräten verschwindet der Palast beim nächsten Sync.
- Der Server behält die älteren, verschlüsselten Änderungen und Medien dieses Palasts, bis du
  **„Daten in der Cloud löschen“** wählst (CLI: `locora cloud erase`). Das löscht alle
  synchronisierten Änderungen und Medien auf dem Server; die Daten auf den Geräten bleiben, und
  angemeldete Geräte laden ihren Stand beim nächsten Sync wieder hoch.
- „Konto endgültig löschen“ entfernt das Konto mit allen Daten vom Server und meldet das Gerät ab.
- Ranglisteneinträge gehören nicht zu den synchronisierten Daten: „Daten in der Cloud löschen“
  lässt sie stehen; „Spielfortschritt löschen“ (oben) entfernt sie.

## Grenzen

- Speicher pro Konto, als Standard des Servers (dort einstellbar): Medien 512 MiB für Gastkonten,
  5 GiB mit Abo; synchronisierte Änderungen 256 MiB bzw. 2 GiB.
- Eine einzelne Änderung über 512 KiB bleibt auf dem Gerät.
- Ist der Speicher voll (507), sagt die App, dass man nicht mehr benötigte Bilder oder Videos
  löschen oder es später versuchen soll. Ist der Server ausgelastet (503) oder kommen zu viele
  Anfragen (429), versucht sie es später noch einmal.
- Wie viel belegt ist, zeigt die Kontoseite.

## Kontoseite web.locora.cloud

Eine kleine statische Seite (im Server-Repository unter `web/`): anmelden, Adresse und
Bestätigung, Abo und Speicher sehen, Passwort ändern, hier oder überall abmelden, Cloud-Daten
oder das Konto löschen. Dort landen auch die Links aus den Mails (`/verify`, `/reset`) und
`/forgot`. Konten werden in der App angelegt, nicht dort.

## Bezahlen nur im Web

Ein Abo wird nur im Web verkauft, nie in den Apps: Google Play und Apple verbieten Preise,
Kaufknöpfe und Links zu einem Bezahlvorgang außerhalb ihres Stores. Die nativen Apps zeigen
deshalb keine Preise, keine Kaufknöpfe und keine Links zu einer Kasse, sondern höchstens neutral
den Stand („Testzeitraum endet am …“, „Abo: aktiv“). `apps/app/ui/src/store_rules.rs` prüft das
in den Tests: Er liest alle UI-Texte in allen fünf Sprachen, die Tauri-Shell und die
Cloud-Meldungen der gemeinsamen Crates und schlägt bei Kaufwörtern, einem Euro-Zeichen oder
einem Link außerhalb einer kurzen Liste fehl (`api.locora.cloud`, `web.locora.cloud/forgot`, das
Issue-Formular auf GitHub …).
Das Bezahlen selbst (Paddle) ist noch nicht gebaut, siehe [`OFFEN.md`](OFFEN.md).

## Wo es im Code steht

- `crates/locora-runtime`: `cloud.rs` (Sitzung, Freischaltung, Server-Adresse
  `https://api.locora.cloud`, `erase_game_call`), `sync_flow.rs` und `cloud_sync.rs` (der Sync,
  auch die erneute Löschbitte), `cloud_key.rs` (der behaltene Schlüssel), `cloud_http.rs` (HTTP
  und Meldungen), `game.rs` (Epoche, Übernahme des alten Stands, Löschen)
- `crates/locora-store/src/outbox.rs`: Ausgang, Zusammenfassen und Übersicht nach Palast
- `crates/locora-crypto`: Verschlüsselung; `crates/locora-cloud-api`: Typen der API
- App: `apps/app/ui/src/cloud_settings.rs`, `cloud_sync_form.rs`, `auto_sync.rs`;
  `apps/app/src-tauri/src/cloud_auto.rs`, `keystore.rs`
- Browser: `crates/locora-web-core/src/cloud.rs`, `cloud_auto.rs` und
  `apps/web/shell/src/cloud.mjs` (fetch, WebCrypto, Argon2 im Worker)
- CLI: `apps/cli/src/cloud.rs` (`locora cloud --help`)
