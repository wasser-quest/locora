#!/usr/bin/env python3
"""Builds the medals of „Mnemotechnik lernen“, the app tour and the first own palace.

Usage: SPLINELY_CLI=splinely-cli python3 assets/ui/src/medaillen.py [OUTPUT_DIR]

One parametric medal (96 x 96, transparent, same build as erfolg-* in ui.py): a star
medal in the colour of the character who teaches the lesson, with a simple topic
symbol on it. Unlocked medals glow, breathe and spin on click; the locked ones are
grey and still. All lessons share one locked medal (erfolg-lernen-gesperrt).
"""
import os, sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "tutorial", "src"))
import tutorial as t  # noqa: E402  (shared Scene helpers and colours)
from tutorial import Scene, ellipse  # noqa: E402

t.OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), ".."))

GREY, GREY_SH, GREY_HI = "#B7B3AA", "#8F8B83", "#D2CEC5"
CREAM = t.CREAM
# The characters' signature colours: base, shadow, light, symbol.
SIMO = ("#3A3D92", "#25276A", "#5258BC", CREAM)
MINA = ("#2A8C88", "#1B6562", "#43ABA6", CREAM)
CAMI = ("#F2B230", "#CF8C12", "#FFD36B", "#5A3A08")
BRUNO = ("#3F7D4A", "#2A5A33", "#5A9E62", CREAM)
HAFIZA = ("#D2487A", "#A3305A", "#E86F99", CREAM)
AMBER = ("#E9C06A", "#B98A3A", "#F6DC9C", "#3A3D92")


def ring(r, w, cx=0, cy=0):
    """A flat ring as one path (outer clockwise, inner counter-clockwise)."""
    k, i = 0.5523, r - w
    o = (f"M{cx - r:g} {cy:g} C {cx - r:g} {cy - r*k:.2f} {cx - r*k:.2f} {cy - r:g} {cx:g} {cy - r:g} "
         f"C {cx + r*k:.2f} {cy - r:g} {cx + r:g} {cy - r*k:.2f} {cx + r:g} {cy:g} "
         f"C {cx + r:g} {cy + r*k:.2f} {cx + r*k:.2f} {cy + r:g} {cx:g} {cy + r:g} "
         f"C {cx - r*k:.2f} {cy + r:g} {cx - r:g} {cy + r*k:.2f} {cx - r:g} {cy:g} Z ")
    n = (f"M{cx - i:g} {cy:g} C {cx - i:g} {cy + i*k:.2f} {cx - i*k:.2f} {cy + i:g} {cx:g} {cy + i:g} "
         f"C {cx + i*k:.2f} {cy + i:g} {cx + i:g} {cy + i*k:.2f} {cx + i:g} {cy:g} "
         f"C {cx + i:g} {cy - i*k:.2f} {cx + i*k:.2f} {cy - i:g} {cx:g} {cy - i:g} "
         f"C {cx - i*k:.2f} {cy - i:g} {cx - i:g} {cy - i*k:.2f} {cx - i:g} {cy:g} Z")
    return o + n


def dot(r, cx, cy):
    return ring(r, r, cx, cy).split(" Z ")[0] + " Z"


# Topic symbols, about 36 x 36 around (0, 0): a list of (path, role); role "fg" is the
# symbol colour, "sh" the medal's shadow tone.
SYMBOLS = {
    "house": [("M0 -17 L19 -1 L13 -1 L13 16 L-13 16 L-13 -1 L-19 -1 Z", "fg"),
              ("M-4 16 L-4 5 L4 5 L4 16 Z", "sh")],
    "eye": [("M-19 0 C -10 -13 10 -13 19 0 C 10 13 -10 13 -19 0 Z", "fg"), (dot(7, 0, 0), "sh")],
    "bolt": [("M5 -19 L-11 3 L-1 3 L-6 19 L11 -5 L1 -5 Z", "fg")],
    "motion": [("M-6 -5 L6 -5 L6 -13 L19 0 L6 13 L6 5 L-6 5 Z", "fg"),
               ("M-19 -5 L-12 -5 L-12 -1 L-19 -1 Z M-19 3 L-10 3 L-10 7 L-19 7 Z", "fg")],
    "heart": [("M0 15 C -22 0 -19 -15 -9 -15 C -4 -15 -1 -11 0 -8 C 1 -11 4 -15 9 -15 C 19 -15 22 0 0 15 Z", "fg")],
    "link": [(ring(10, 4, -7, 0), "fg"), (ring(10, 4, 7, 0), "fg")],
    "bulb": [(dot(11, 0, -5), "fg"), ("M-6 7 L6 7 L6 15 L-6 15 Z", "fg"), ("M-4 17 L4 17 L4 19 L-4 19 Z", "fg")],
    "hash": [("M-9 -17 L-4 -17 L-4 17 L-9 17 Z M4 -17 L9 -17 L9 17 L4 17 Z "
              "M-17 -9 L17 -9 L17 -4 L-17 -4 Z M-17 4 L17 4 L17 9 L-17 9 Z", "fg")],
    "pin": [("M0 19 C -6 9 -14 1 -14 -5 C -14 -14 -7 -18 0 -18 C 7 -18 14 -14 14 -5 C 14 1 6 9 0 19 Z", "fg"),
            (dot(5, 0, -5), "sh")],
    "steps": [(ellipse(6, 9), "fg-l"), (ellipse(6, 9), "fg-r")],
    "clock": [(ring(17, 4), "fg"), ("M-2 -11 L2 -11 L2 -2 L10 -2 L10 2 L-2 2 Z", "fg")],
    "cycle": [("M-15 2 C -15 -10 -6 -16 3 -16 L3 -21 L13 -13 L3 -5 L3 -10 C -4 -10 -9 -5 -9 2 Z "
               "M15 -2 C 15 10 6 16 -3 16 L-3 21 L-13 13 L-3 5 L-3 10 C 4 10 9 5 9 -2 Z", "fg")],
    "bang": [("M-4 -19 L4 -19 L3 6 L-3 6 Z", "fg"), (dot(4.5, 0, 13), "fg")],
    "temple": [("M-17 -14 L0 -22 L17 -14 Z M-15 -12 L15 -12 L15 -8 L-15 -8 Z M-12 -8 L-7 -8 L-7 12 L-12 12 Z "
                "M-2.5 -8 L2.5 -8 L2.5 12 L-2.5 12 Z M7 -8 L12 -8 L12 12 L7 12 Z M-17 12 L17 12 L17 17 L-17 17 Z", "fg")],
    "lens": [(ring(11, 4, -4, -4), "fg"), ("M3 7 L7 3 L18 14 L14 18 Z", "fg")],
    "cup": [("M-13 -17 L13 -17 L13 -6 C 13 4 7 8 0 8 C -7 8 -13 4 -13 -6 Z M-3 8 L3 8 L3 13 L-3 13 Z "
             "M-10 13 L10 13 L10 18 L-10 18 Z", "fg")],
    "key": [(ring(9, 4, -8, -8), "fg"), ("M-3 -3 L1 -7 L19 11 L15 15 L13 13 L10 16 L7 13 L10 10 Z", "fg")],
    "flag": [("M-11 -19 L-7 -19 L-7 19 L-11 19 Z M-7 -19 L16 -12 L-7 -4 Z", "fg")],
    "crown": [("M-19 12 L-19 -11 L-9 -1 L0 -18 L9 -1 L19 -11 L19 12 Z M-19 15 L19 15 L19 19 L-19 19 Z", "fg")],
    "castle": [("M-18 19 L-18 -13 L-12 -13 L-12 -7 L-6 -7 L-6 -13 L6 -13 L6 -7 L12 -7 L12 -13 L18 -13 L18 19 "
                "L5 19 L5 8 C 5 2 -5 2 -5 8 L-5 19 Z", "fg")],
}

# Lesson id → (teacher colours, symbol, title). Ids match apps/app/ui/src/learn.rs.
LESSONS = [
    ("basics", SIMO, "house", "Grundlagen"),
    ("visualize", CAMI, "eye", "Visualisieren"),
    ("exaggerate", CAMI, "bolt", "Übertreiben"),
    ("motion", CAMI, "motion", "Bewegung"),
    ("senses", CAMI, "heart", "Alle Sinne"),
    ("link", CAMI, "link", "Verknüpfen"),
    ("abstract", CAMI, "bulb", "Abstraktes greifbar machen"),
    ("numbers", HAFIZA, "hash", "Zahlen merken"),
    ("palace", BRUNO, "castle", "Einen Palast wählen"),
    ("loci", BRUNO, "pin", "Orte auswählen"),
    ("route", SIMO, "steps", "Eine Route durchgehen"),
    ("review", HAFIZA, "clock", "Wiederholen"),
    ("reuse", BRUNO, "cycle", "Paläste wiederverwenden"),
    ("mistakes", MINA, "bang", "Häufige Fehler"),
    ("history", SIMO, "temple", "Geschichte"),
    ("research", MINA, "lens", "Forschung"),
    ("sport", HAFIZA, "cup", "Gedächtnissport"),
]


def medal(name, title, tips, colours, symbol, description, unlocked=True):
    fill, shadow, light, fg = colours if unlocked else (GREY, GREY_SH, GREY_HI, GREY_HI)
    s = Scene(name, 96, 96, title)
    if unlocked:
        s.glow("halo", 48, 48, 46, strength=0.5)
    m = s.add("star", "medal", x=48, y=48, radius=38, tips=tips, inner=0.78, fill=fill, pivot="center")
    s.crescent(m, -4, -4, shadow)
    s.crescent(m, 3, 3, light)
    if symbol:
        for i, (d, role) in enumerate(SYMBOLS[symbol]):
            colour = shadow if role == "sh" else fg
            dx = {"fg-l": -7, "fg-r": 7}.get(role, 0)
            dy = {"fg-l": 5, "fg-r": -6}.get(role, 0)
            s.add("path", f"sym{i}", d=d, tx=48 + dx, ty=48 + dy, fill=colour, scale_x=0.8, scale_y=0.8)
    if unlocked:
        for prop in ("scaleX", "scaleY"):
            extra = " --autoplay --loop" if prop == "scaleX" else ""
            s.steps += [
                f"keyframe --clip breathe --target medal --property {prop} --time 1.2 --value 1.06 --easing ease-in-out{extra}",
                f"keyframe --clip breathe --target medal --property {prop} --time 2.4 --value 1 --easing ease-in-out",
            ]
        s.steps += [
            "clip add --id spin --duration 0.7",
            "keyframe --clip spin --target medal --property rotation --time 0.7 --value 360 --easing ease-in-out",
            "on --event click --target medal --play spin",
            f'a11y --node medal --label "{title}: klicken, dann dreht sich die Medaille"',
        ]
    s.finish(description)


if __name__ == "__main__":
    os.makedirs(t.OUT, exist_ok=True)
    for lesson, colours, symbol, title in LESSONS:
        medal(f"erfolg-lernen-{lesson}", title, 8, colours, symbol,
              f"Eine Medaille mit acht Spitzen: die Lektion „{title}“ ist geschafft.")
    medal("erfolg-lernen-gesperrt", "Lektion", 8, None, None,
          "Eine graue Medaille: diese Lektion ist noch nicht geschafft.", unlocked=False)
    medal("erfolg-lernen-alle", "Alle Techniken gelernt", 10, AMBER, "crown",
          "Eine goldene Medaille mit Krone: alle Lektionen sind geschafft.")
    medal("erfolg-lernen-alle-gesperrt", "Alle Techniken gelernt", 10, None, None,
          "Eine graue Medaille: noch nicht alle Lektionen sind geschafft.", unlocked=False)
    medal("erfolg-tour", "Tour gemeistert", 7, SIMO, "flag",
          "Eine indigoblaue Medaille mit Fahne: die App-Tour ist geschafft.")
    medal("erfolg-tour-gesperrt", "Tour gemeistert", 7, None, None,
          "Eine graue Medaille: die App-Tour ist noch nicht geschafft.", unlocked=False)
    medal("erfolg-palast", "Erster eigener Palast", 9, BRUNO, "key",
          "Eine grüne Medaille mit Schlüssel: der erste eigene Palast ist gebaut und durchlaufen.")
    medal("erfolg-palast-gesperrt", "Erster eigener Palast", 9, None, None,
          "Eine graue Medaille: der erste eigene Palast fehlt noch.", unlocked=False)
