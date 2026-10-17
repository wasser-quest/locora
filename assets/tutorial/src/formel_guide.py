#!/usr/bin/env python3
"""Build the six animated locus pictures for the formula guide.

Usage: SPLINELY_CLI=splinely-cli python3 assets/tutorial/src/formel_guide.py
The shared Scene helper keeps the same palette and Splinely format as the
"Locora lernen" palace. Existing room backdrops are reused.
"""

import os
import tutorial as art

art.OUT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "formel-guide"))


def card(scene, title, subtitle):
    scene.add("path", "card", d=art.rounded(320, 248, 24), tx=32, ty=68,
              fill=art.CREAM)
    scene.add("path", "shadow", d=art.rounded(296, 12, 6), tx=44, ty=292,
              fill=art.CREAM_SH)
    scene.add("text", "title", x=56, y=122, text=title,
              font_size=23, fill=art.INK)
    scene.add("text", "subtitle", x=56, y=270, text=subtitle,
              font_size=21, fill=art.INK)


def token(scene, id, x, y, label, width=230):
    scene.glow(id + "_glow", x + width / 2, y + 28, 66, 0.48)
    scene.add("path", id + "_plate", d=art.rounded(width, 60, 14),
              tx=x, ty=y, fill=art.INDIGO)
    scene.add("text", id + "_text", x=x + 18, y=y + 40, text=label,
              font_size=23, fill=art.CREAM)
    scene.pulse(id, [id + "_glow"], low=0.28, period=3.2)


def eingabe():
    s = art.Scene("eingabe", 384, 384, "Eingabe lesen")
    card(s, "Eingaben", "Name → Wert")
    token(s, "input", 74, 161, "$zahl", 236)
    s.finish("Ein Eingabebuch mit leuchtendem Dollarzeichen vor dem Namen zahl.")


def rechnen():
    s = art.Scene("rechnen", 384, 384, "Mit Eingaben rechnen")
    card(s, "Umwandeln", "Zahl + 1")
    token(s, "integer", 53, 161, "Integer($zahl)", 278)
    s.finish("Ein Rechentisch zeigt Integer von zahl; die Texteingabe wird zur Zahl.")


def tabelle():
    s = art.Scene("tabelle", 384, 384, "Tabellenzeile lesen")
    card(s, "Register Major", "Schlüssel 4 → Reh")
    token(s, "major", 66, 161, 'Major["4"]', 252)
    s.finish("Ein Register zeigt den Zugriff auf die Zeile vier der Major-Tabelle.")


def bindung():
    s = art.Scene("bindung", 384, 384, "Inhalt binden")
    card(s, "Ort im Palast", "Formel → Inhalt")
    token(s, "item", 86, 161, "item", 212)
    s.finish("Das Feld item leuchtet auf: eine Bindung berechnet den Inhalt eines Orts.")


def aktiv():
    s = art.Scene("aktiv", 384, 384, "Aktiv-Bedingung")
    card(s, "Bedingung", "true → üben")
    token(s, "active", 69, 161, "active: > 2", 246)
    s.finish("Eine leuchtende Weiche zeigt: Ein Ort ist aktiv, wenn die Bedingung true ergibt.")


def quelle():
    s = art.Scene("quelle", 384, 384, "Bindungen in der Quelle")
    card(s, "locora.toml", "Formeln im Palast")
    token(s, "bindings", 63, 161, "bindings = { ... }", 258)
    s.finish("Ein Quellblatt mit bindings: Hier stehen Formeln für Ortseigenschaften.")


if __name__ == "__main__":
    os.makedirs(art.OUT, exist_ok=True)
    for build in (eingabe, rechnen, tabelle, bindung, aktiv, quelle):
        build()
