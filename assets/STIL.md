# Locora-Stil

Dieser Leitfaden beschreibt, wie Figuren und Illustrationen in Locora aussehen: ein freundlicher, farbkräftiger Zeichentrick-Look **ohne Konturen**, nur aus Flächen mit **Cel-Shading**. Er muss klein (App-Icon, Benachrichtigung) genauso funktionieren wie groß (Startbildschirm, Tutorial).

Alle Grafiken entstehen mit `splinely-cli` als `.sply`-Dateien und werden in der App über die Splinely-Runtime abgespielt (siehe [`README.md`](README.md)).

---

## 1. Grundidee

- **Keine Konturen.** Formen werden nur durch Farbflächen und Tonwerte getrennt. Wo zwei gleichfarbige Teile übereinanderliegen (Ärmel vor Jacke), trennt sie ein Schatten- oder Lichtton – nie eine Linie.
- **Cel-Shading:** jede Fläche besteht aus einem Grundton, **einer** harten Schattenfläche und höchstens **einer** harten Lichtkante. Keine weichen Verläufe auf Haut, Haar oder Kleidung.
- **Kindchenschema, ruhig und freundlich:** großer Kopf, Ganzkörperfigur etwa **2,5–3 Köpfe** hoch. Figuren lächeln leicht und wirken zugewandt.
- **Wenige, klare Formen:** jede Form muss bei 64 px noch lesbar sein.
- **Leuchten als Erinnerung:** Licht steht in Locora für *Erinnern*. Loci-Punkte, das Glühwürmchen und die Laterne tragen die Markenfarbe Limettengrün und sind die einzigen Stellen mit weichem Verlauf.

## 2. Licht und Schatten

- **Lichtquelle oben links.** Schatten liegen unten rechts, Lichtkanten oben links.
- **Schatten folgen der Silhouette.** Eine Schattenfläche entsteht, indem man eine Kopie der Form nach oben links verschiebt und von der Form abzieht: übrig bleibt eine Sichel am rechten/unteren Rand (`crescent` im Bauskript). Die Lichtkante entsteht genauso mit Verschiebung nach unten rechts.
- **Schlagschatten:** Haar wirft einen Schatten auf die Stirn, das Kinn auf den Hals. Dazu wird die schattenwerfende Form leicht nach unten verschoben und mit der Zielfläche geschnitten (`cast`).
- Typische Verschiebungen bei 512 px Breite: Schatten 12–18 px, Lichtkante 8–12 px, Schlagschatten 10–14 px.

## 3. Proportionen (Ganzkörper, 512 × 768)

| Teil | Maß |
| --- | --- |
| Kopf inkl. Haar | ≈ 40 % der Höhe |
| Gesicht | ≈ 184 px breit, breiter, kantiger Kiefer, deutliches Kinn |
| Schultern | ≈ 256 px breit, Oberkörper V-förmig |
| Arme | Kapseln, 46 px breit; Hände als Kreise (Ø 36 px) |
| Beine | Kapseln, 54 px breit; Schuhe kräftig, abgerundet |

## 4. Gesicht

- **Augen mandelförmig**, eher breit als hoch. Iris mittelgroß mit Verlauf dunkel oben → hell unten, Pupille klein, **ein** Glanzlicht oben links und ein kleiner Punkt unten rechts.
- **Oberlid** als kräftige, dunkle Fläche (kein dichter Wimpernkranz, kein Schwung). Unterlid nur als schmale, dunklere Hautfläche angedeutet.
- **Brauen** kräftig, fast gerade, als flächige Keile im dunklen Haarton.
- **Nase** als kleine, dunklere Hautfläche (Winkel), **Mund** als schmale dunkle Fläche mit leichtem Lächeln, ein Mundwinkel etwas höher.
- **Ohren** sichtbar, mit dunklerer Innenfläche; das abgewandte Ohr liegt im Schatten.
- Kein Rouge, keine Wangenstriche.

Gesichtsmerkmale (Lider, Brauen, Mund) sind **Formen**, keine Konturen: sie haben eine eigene Fläche und Dicke.

## 5. Haare (Afro)

- **Kompakter, runder Afro:** großer Kreis plus gleichmäßig verteilte Wölbungen, zu **einer** Form vereinigt.
- **Haarlinie:** klare, fast gerade Kante über der Stirn mit sauberen Ecken an den Schläfen (aus der Haarform ausgeschnitten). Die Ohren bleiben frei.
- **Cel-Shading:** Schatten-Sichel unten rechts, Lichtkante oben links.
- **Textur:** verstreute kleine Locken-Häkchen als Flächen, links im Lichtton, rechts im Schattenton, mit zufälliger Drehung und Größe. Nie in Reihen anordnen.

## 6. Kleidung

- **Jacke** in Indigo mit kurzem Kragen (eine Kragenseite im Licht-, eine im Schattenton), Reißverschluss und Bündchen in Limettengrün.
- **Loci-Anstecker:** drei leuchtende Punkte in einer schrägen Reihe auf der Brust.
- **Hose** in dunklem Indigo, **Schuhe** cremeweiß mit dunklerer Sohle und limettengrünem Streifen.

## 7. Farben

### Simo

| Rolle | Grundton | Schatten | Licht / tief |
| --- | --- | --- | --- |
| Haut | `#C88B5E` | `#A86C44` | Licht `#DDA77C`, tief `#8E5634` |
| Haar | `#3E2416` | `#24140B` | `#5E3A24` |
| Jacke | `#3A3D92` | `#25276A` | `#5258BC` |
| Hose | `#2A2C70` | `#1A1B4C` | – |
| Shirt / Schuhe | `#F3EEE3` | `#D3CBB9` | – |
| Gesichtsmerkmale | `#2A160C` | – | – |

### Augen (Bernstein)

| Rolle | Farbe |
| --- | --- |
| Iris oben → unten | `#3E1904` → `#9A5716` → `#E9C06A` |
| Pupille | `#1E0B02` |
| Augenweiß | `#FBF6EE` (warmes Weiß, nie `#FFFFFF`) |

### Marke und Leuchten

| Rolle | Farbe |
| --- | --- |
| Limettengrün (Leuchten) | `#DDF76A` |
| Limettengrün (Bündchen, Streifen) | `#C9E85A` |
| Leuchtkern | `#FFFFF0` |
| Glühwürmchen-Kopf | `#5A63D8` → `#2E2F78` |

Leuchten nutzt immer denselben Aufbau: radialer Hof (`rgba(230,255,120,0.8)` → transparent) und darauf ein Kern mit Verlauf `#FFFFF0` → `#DDF76A` → `#A8CF2E`.

## 8. Locora-Motive

- **Laterne:** Glaskörper mit Leuchtverlauf, Kappe und Boden in Indigo; das Licht pulsiert.
- **Loci-Anstecker:** drei Punkte, die in der Animation **nacheinander** (1 → 2 → 3) aufleuchten – die Loci-Methode in einem Bild.
- **Glühwürmchen:** Begleiter mit indigo Kopf, großen weißen Augen, transparenten Flügeln und leuchtendem Hinterleib.

## 9. Bewegung

| Bewegung | Werte |
| --- | --- |
| Atmen (Rumpf-Bone) | 3 px hoch und zurück, 3,6 s, ease-in-out, Endlosschleife |
| Blinzeln | alle 3,8 s, geschlossen 0,13 s, Überblendung 0,06 s; geschlossenes Auge als gebogene Lidfläche |
| Blick | Iris, Pupille und Glanzlichter folgen dem Zeiger um höchstens 3 px |
| Laterne | Arm schwingt um 4°, die Laterne pendelt um 8° nach (2,4 s) |
| Glühwürmchen | schwebt 10 px auf und ab (2,6 s), Flügelschlag 0,2 s, Leuchten pulsiert (2,4 s) |
| Loci | leuchten nacheinander auf (Abstand 0,5 s), halten, verblassen gemeinsam (Zyklus 3 s) |

Bewegungen sind **klein und ruhig**; nichts blinkt hektisch.

**Rig:** ein Rumpf-Bone (Atmen), je ein Arm-Bone an der Schulter; Dinge in der Hand hängen an einem eigenen Bone, der die Armdrehung ausgleicht und beim Schwingen nachpendelt. Schattenflächen eines Teils hängen am selben Bone wie das Teil.

## 10. Arbeitsweise mit Splinely

- Figuren werden **per Skript** gebaut: `assets/mascot/src/simo.py` ruft `splinely-cli` auf, erzeugt Formen, Schatten (`crescent`, `cast`), Rig und Animationen.
- Nach jeder Änderung: `splinely-cli check`, dann `splinely-cli render … --width 1024` und das Bild ansehen; Gesicht vergrößert, geschlossene Augen und Bewegungen zusätzlich prüfen.
- Exporte (`.svg`, `.gif`, `.png`) werden immer aus der `.sply`-Quelle erzeugt, nie von Hand bearbeitet.

## 11. Checkliste

- [ ] Keine einzige Kontur – Trennung nur über Tonwerte
- [ ] Jede Fläche: Grundton + eine harte Schattenfläche (unten rechts), höchstens eine Lichtkante (oben links)
- [ ] Schatten bleiben innerhalb der Silhouette
- [ ] Glanzlichter in beiden Augen oben links
- [ ] Gesicht klar lesbar: kräftige Brauen, mandelförmige Augen, sichtbare Ohren
- [ ] Mindestens ein leuchtender Locus im Bild
- [ ] Bei 64 px noch erkennbar
