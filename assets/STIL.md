# Locora-Stil

Dieser Leitfaden beschreibt, wie Figuren und Illustrationen in Locora aussehen. Ziel ist ein freundlicher, farbkräftiger Zeichentrick-Look mit klaren Formen, der in klein (App-Icon, Benachrichtigung) genauso funktioniert wie groß (Startbildschirm, Tutorial).

Alle Grafiken entstehen mit `splinely-cli` als `.sply`-Dateien und werden in der App über die Splinely-Runtime abgespielt (siehe [`README.md`](README.md)).

---

## 1. Grundidee

- **Kindchenschema:** großer Kopf, große Augen, kleiner Mund, kleine Nase. Der Kopf nimmt etwa die Hälfte der Bildhöhe ein (Brustbild).
- **Ruhig und freundlich:** Figuren lächeln leicht und wirken neugierig, nie streng. Brauen sind dünn und leicht gewölbt; der innere Brauenpunkt liegt nie tiefer als der äußere.
- **Wenige, klare Formen:** lieber eine gut gesetzte Fläche als viele kleine Details. Jede Form muss bei 64 px noch lesbar sein.
- **Leuchten als Erinnerung:** Licht steht in Locora für *Erinnern*. Leuchtende Punkte (Loci), das Glühwürmchen und die Brosche tragen die Markenfarbe Limettengrün.

## 2. Linien

| Element | Farbe | Stärke (bei 512 px) |
| --- | --- | --- |
| Außenkontur (Haare, Gesicht, Kleidung) | `#24140E` | 5–6 |
| Innenkonturen (Pony über dunklem Haar) | `#2A170E` | 4–4,5 |
| Feine Linien (Brauen, Unterlid, Mund, Nase) | `#24140E` … `#5A2C14` | 2,5–3,5 |
| Dekorkanten (Kragen) | `#C9E85A` | 4 |

- Konturen sind **dunkelbraun, nie reines Schwarz**. Das wirkt weicher und passt zu warmen Hauttönen.
- Feine Linien im Gesicht werden zur Mitte hin heller und dünner (Nase `#5A2C14`, Mund `#3A1A0E`).

## 3. Farben

### Haut (Beispiel Mnemo)

| Rolle | Farbe |
| --- | --- |
| Licht (Stirn, Kinn) | `#B97A4C` |
| Grundton | `#9A5B32` |
| Schatten (Cel) | `#7A4322` |

### Haare (Afro)

| Rolle | Farbe |
| --- | --- |
| Mitte / Licht | `#70432A` |
| Grundton | `#47291A` |
| Rand / Tiefe | `#2A170E` |
| Locken-Häkchen | `#8A5536` (Deckkraft 0,8) |
| Glanz-Sichel | `rgba(175,115,80,0.32)` |

### Augen

| Rolle | Farbe |
| --- | --- |
| Iris oben | `#2E0F52` |
| Iris Mitte | `#6E35A8` |
| Iris unten | `#C88AF0` |
| Pupille | `#1C0A30` |
| Unterer Iris-Schimmer | `#F3D2FF` (Deckkraft 0,85) |
| Augenweiß | `#FFF8F2` (warmes Weiß, nie `#FFFFFF`) |
| Wimpern | `#1A0E0A` |

### Marke

| Rolle | Farbe |
| --- | --- |
| Indigo hell | `#5A63D8` / `#3A3D92` |
| Indigo dunkel | `#2E2F78` / `#1E1F55` |
| Limettengrün (Leuchten) | `#DDF76A` |
| Limettengrün Kante | `#C9E85A` |
| Leuchtkern | `#FFFFF0` |

### Wangen

Rouge `#F2728A` mit Deckkraft 0,42 als liegende Ellipse, darauf drei kurze schräge Striche `#C9455F`.

## 4. Schattierung

- **Cel-Shading:** eine Grundfläche plus **eine** harte Schattenfläche. Keine weichen Airbrush-Verläufe auf der Haut.
- Ausnahmen mit weichem Verlauf: das **Leuchten** (radiale Verläufe, die nach außen transparent werden) und ein sehr flacher Licht-Verlauf von oben auf Gesicht und Haar.
- Schatten fallen **unter den Pony** (sanfter Bogen, keine Spitze in der Mitte) und auf die **lichtabgewandte Wangenseite** (rechts).
- Lichtquelle ist **oben links**: Glanzlichter in den Augen sitzen links oben, die Glanz-Sichel im Haar ebenfalls.

## 5. Augen – Aufbau von hinten nach vorn

1. Augenweiß: oben flach gewölbt, unten rund.
2. Iris: hohe Ellipse (Verhältnis etwa 20 : 26) mit Verlauf dunkel oben → hell unten und dünner dunkler Kontur.
3. Innerer Iris-Ring: feine, halbtransparente helle Linie.
4. Pupille: dunkle Ellipse, leicht nach oben versetzt.
5. Unterer Schimmer: heller Halbmond im unteren Iris-Drittel.
6. Glanzlichter: **ein großes Oval** oben links, **ein kleiner Punkt** unten rechts, **ein vierzackiger Funkelstern** oben rechts.
7. Oberes Lid: kräftiger, nach außen dicker werdender Wimpernbogen mit **zwei Schwüngen am äußeren Augenwinkel**. Der Schwung zeigt immer nach außen.
8. Unterlid: kurzer, dünner Bogen.

Blinzeln: Alle Augenteile werden ausgeblendet, ein geschlossener Lidbogen (nach unten gewölbt) eingeblendet.

## 6. Haare (Afro)

- **Silhouette:** großer Kreis mit gleichmäßig verteilten, leicht unterschiedlich großen Wölbungen (wolkenartig), per Boolescher Vereinigung zu **einer** Form verbunden.
- **Pony:** eigene, wolkige Form über der Stirn, die den Haaransatz verdeckt. Der untere Rand besteht aus runden Wölbungen; die Brauen schauen knapp darunter hervor.
- **Textur:** verstreute kleine Locken-Häkchen mit zufälliger Drehung und Größe (0,75–1,15). Nie in Reihen anordnen.
- **Glanz:** eine einzige Sichel, die der oberen Rundung folgt.

## 7. Kleidung

- Indigo-Oberteil mit hohem Kragen; die Kragenspitzen schwingen nach außen und haben eine Kante in Limettengrün.
- In der Mitte sitzt eine **rautenförmige Leucht-Brosche** – ein Locus.

## 8. Locora-Motive

- **Loci-Haarspange:** drei leuchtende Punkte in einer schrägen Reihe, die in der Animation **nacheinander** (1 → 2 → 3) aufleuchten – die Loci-Methode in einem Bild.
- **Glühwürmchen:** Begleiter mit indigo Kopf, großen weißen Augen, transparenten Flügeln und leuchtendem Hinterleib.
- Leuchten nutzt immer denselben Aufbau: radialer Hof (`rgba(230,255,120,0.8)` → transparent) und darauf ein Kern mit Verlauf `#FFFFF0` → `#DDF76A` → `#A8CF2E` und dunkler Kontur.

## 9. Bewegung

| Bewegung | Werte |
| --- | --- |
| Atmen (ganze Figur als Gruppe) | 4 px hoch und zurück, 3,6 s, ease-in-out, Endlosschleife |
| Blinzeln | alle 3,4 s, geschlossen 0,13 s, Überblendung 0,06 s |
| Blick | Iris, Pupille und Glanzlichter folgen dem Zeiger um höchstens 3,5 px |
| Glühwürmchen | schwebt 10 px auf und ab (2,6 s), Flügelschlag 0,2 s, Leuchten pulsiert (2,4 s) |
| Loci | leuchten nacheinander auf (Abstand 0,5 s), halten, verblassen gemeinsam (Zyklus 3 s) |

Bewegungen sind **klein und ruhig**. Nichts blinkt hektisch; nichts bewegt sich schneller als der Flügelschlag.

## 10. Arbeitsweise mit Splinely

- Figuren werden **per Skript** gebaut (`assets/mascot/src/build.py` erzeugt die Befehle, `make.sh` führt sie mit `splinely-cli batch` aus). So bleibt jede Grafik reproduzierbar und überprüfbar.
- Ganze Figuren werden als **Gruppe** animiert (Atmen, Schweben), Einzelteile direkt.
- Nach jeder Änderung: `splinely-cli check`, dann `splinely-cli render … --width 1024` und das Bild ansehen, Gesicht zusätzlich vergrößert prüfen.
- Exporte (`.svg`, `.gif`, `.png`) werden immer aus der `.sply`-Quelle erzeugt, nie von Hand bearbeitet.

## 11. Checkliste

- [ ] Kontur dunkelbraun, nicht schwarz
- [ ] Glanzlichter in beiden Augen auf derselben Seite (oben links)
- [ ] Wimpernschwung zeigt nach außen
- [ ] Innere Brauenenden nicht tiefer als äußere (freundlicher Ausdruck)
- [ ] Genau eine harte Schattenfläche pro Hautbereich
- [ ] Mindestens ein leuchtender Locus im Bild
- [ ] Bei 64 px noch erkennbar
