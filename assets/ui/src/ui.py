#!/usr/bin/env python3
"""Builds the app's UI graphics with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/ui/src/ui.py [OUTPUT_DIR]

Style: assets/STIL.md (no outlines, cel shading, light from the top left,
lime glow for remembering). Every file moves a little, never hectically.
- Tile icons, 96 x 96, transparent: kachel-core, kachel-review, kachel-notify,
  kachel-tutorial, kachel-gamification, kachel-plugin (any other plugin).
- Journey feedback, 192 x 192, transparent: richtig, nochmal, geschafft.
- Room kinds, 48 x 48, transparent: raum-leer, raum-bild, raum-video.
"""
import math, os, sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "tutorial", "src"))
import tutorial as t  # noqa: E402  (shared Scene helpers and colours)
from tutorial import Scene, ellipse, rounded  # noqa: E402

t.OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), ".."))
INDIGO, INDIGO_SH, INDIGO_HI = t.INDIGO, t.INDIGO_SH, t.INDIGO_HI
CREAM, CREAM_SH, LIME, AMBER = t.CREAM, t.CREAM_SH, t.LIME, t.AMBER
AMBER_SH, AMBER_HI = "#B98A3A", "#F6DC9C"


def swing(s, clip, target, degrees, period):
    """A calm back-and-forth rotation (needs --pivot center on the target)."""
    s.steps += [
        f"keyframe --clip {clip} --target {target} --property rotation --time 0.01 --value {-degrees} --autoplay --loop",
        f"keyframe --clip {clip} --target {target} --property rotation --time {period/2:g} --value {degrees} --easing ease-in-out",
        f"keyframe --clip {clip} --target {target} --property rotation --time {period:g} --value {-degrees} --easing ease-in-out",
    ]


def breathe(s, clip, target, scale=1.12, period=2.4):
    for prop in ("scaleX", "scaleY"):
        extra = " --autoplay --loop" if prop == "scaleX" else ""
        s.steps += [
            f"keyframe --clip {clip} --target {target} --property {prop} --time {period/2:g} --value {scale} --easing ease-in-out{extra}",
            f"keyframe --clip {clip} --target {target} --property {prop} --time {period:g} --value 1 --easing ease-in-out",
        ]


# ---------------------------------------------------------------- tile icons (96 x 96)

def kachel_core():
    s = Scene("kachel-core", 96, 96, "Paläste")
    arch = s.add("path", "arch", d="M-30 34 L-30 -6 C -30 -30 -16 -40 0 -40 C 16 -40 30 -30 30 -6 L30 34 Z",
                 tx=48, ty=52, fill=INDIGO)
    s.crescent(arch, -6, -5, INDIGO_SH)
    s.crescent(arch, 4, 4, INDIGO_HI)
    s.add("path", "door", d="M-17 34 L-17 2 C -17 -14 -8 -22 0 -22 C 8 -22 17 -14 17 2 L17 34 Z", tx=48, ty=52, fill=t.INDIGO_DEEP)
    groups = []
    for i, (x, y) in enumerate([(40, 76), (56, 62), (44, 46)]):
        s.glow(f"g{i}", x, y, 9)
        s.light(f"l{i}", x, y, 4)
        groups.append([f"g{i}", f"l{i}"])
    s.sequence("loci", groups, step=0.5, hold=2.4, cycle=3.0, low=0.25)
    s.finish("Ein Torbogen mit drei Loci, die nacheinander aufleuchten.")


def kachel_review():
    s = Scene("kachel-review", 96, 96, "Wiederholung")
    outer = s.add("circle", "o", x=48, y=48, radius=34)
    inner = s.add("circle", "i", x=48, y=48, radius=24)
    ring = s.cli("arrange", "boolean", s.file, "--op", "difference", "--nodes", f"{outer},{inner}")["id"]
    gap = s.add("rect", "gap", x=48, y=8, width=40, height=26)
    ring = s.cli("arrange", "boolean", s.file, "--op", "difference", "--nodes", f"{ring},{gap}")["id"]
    s.update(ring, fill=INDIGO, pivot="center")
    s.crescent(ring, -5, -5, INDIGO_SH)
    s.add("path", "head", d="M0 -12 L16 0 L0 12 Z", tx=46, ty=19, fill=INDIGO)
    s.glow("glow", 48, 48, 22)
    s.light("core", 48, 48, 9)
    s.pulse("core", ["glow"], low=0.4, period=2.4)
    s.finish("Ein Pfeil im Kreis um einen leuchtenden Locus: wiederholen, was fällig ist.")


def kachel_notify():
    s = Scene("kachel-notify", 96, 96, "Erinnerung")
    bell = s.add("path", "bell", d="M-26 18 C -22 12 -20 6 -20 -6 C -20 -22 -11 -30 0 -30 C 11 -30 20 -22 20 -6 C 20 6 22 12 26 18 Z",
                 tx=48, ty=48, fill=INDIGO, pivot="center")
    s.crescent(bell, -6, -5, INDIGO_SH)
    s.crescent(bell, 4, 4, INDIGO_HI)
    s.add("rect", "rim", x=20, y=64, width=56, height=7, rx=3.5, fill=INDIGO_SH)
    s.glow("glow", 48, 76, 14)
    s.light("clapper", 48, 76, 6)
    s.add("circle", "knob", x=48, y=16, radius=4, fill=INDIGO_SH)
    swing(s, "ring", bell, 6, 2.8)
    s.pulse("clapper", ["glow"], low=0.35, period=2.8)
    s.finish("Eine Glocke mit leuchtendem Klöppel: Erinnerungen.")


def kachel_tutorial():
    s = Scene("kachel-tutorial", 96, 96, "Tutorial")
    s.add("rect", "handle", x=40, y=8, width=16, height=8, rx=4, fill=INDIGO_SH)
    cap = s.add("path", "cap", d=rounded(36, 12, 4), tx=30, ty=16, fill=INDIGO)
    s.crescent(cap, -3, -3, INDIGO_SH)
    s.glow("halo", 48, 48, 34, strength=0.7)
    s.add("rect", "glass", x=32, y=28, width=32, height=40, rx=6, pivot="center",
          gradient=f"radial {t.GLOW_CORE}@0 {LIME}@0.55 #A8CF2E@1")
    s.add("rect", "shine", x=36, y=32, width=5, height=20, rx=2.5, fill="white", opacity=0.55)
    base = s.add("path", "base", d=rounded(40, 10, 4), tx=28, ty=68, fill=INDIGO)
    s.crescent(base, -3, -3, INDIGO_SH)
    s.pulse("light", ["halo"], low=0.45, period=2.4)
    s.finish("Eine Laterne, deren Licht pulsiert: Locora lernen.")


def kachel_gamification():
    s = Scene("kachel-gamification", 96, 96, "Erfolge")
    s.glow("halo", 48, 50, 38, strength=0.55)
    star = s.add("star", "star", x=48, y=50, radius=32, tips=5, inner=0.45, fill=AMBER, pivot="center")
    s.crescent(star, -6, -6, AMBER_SH)
    s.crescent(star, 4, 4, AMBER_HI)
    for i, (x, y, r) in enumerate([(20, 22, 4), (78, 20, 3), (80, 70, 3.5)]):
        s.light(f"spark{i}", x, y, r)
    s.sequence("twinkle", [["spark0"], ["spark1"], ["spark2"]], step=0.6, hold=2.4, cycle=3.2, low=0.15)
    breathe(s, "breathe", "halo", scale=1.1, period=2.4)
    s.finish("Ein Stern mit funkelnden Lichtern: Punkte, Serien und Erfolge.")


def kachel_plugin():
    s = Scene("kachel-plugin", 96, 96, "Plugin")
    parts = [s.add("path", "body", d=rounded(48, 48, 10), tx=20, ty=28),
             s.add("circle", "knob_top", x=44, y=26, radius=10),
             s.add("circle", "knob_right", x=70, y=52, radius=10)]
    piece = s.cli("arrange", "boolean", s.file, "--op", "union", "--nodes", ",".join(parts))["id"]
    s.update(piece, fill=INDIGO)
    s.crescent(piece, -6, -6, INDIGO_SH)
    s.crescent(piece, 4, 4, INDIGO_HI)
    s.glow("glow", 44, 52, 14)
    s.light("dot", 44, 52, 6)
    s.pulse("dot", ["glow"], low=0.4, period=2.6)
    s.finish("Ein Puzzleteil mit leuchtendem Punkt: eine Erweiterung.")


# ---------------------------------------------------------------- journey feedback (192 x 192)

def richtig():
    s = Scene("richtig", 192, 192, "Richtig")
    s.glow("halo", 96, 96, 92, strength=0.85)
    s.add("circle", "disc", x=96, y=96, radius=58, pivot="center",
          gradient=f"radial {t.GLOW_CORE}@0 {LIME}@0.6 #A8CF2E@1")
    check = s.add("path", "check", d="M-34 2 L-22 -10 L-8 6 L24 -30 L36 -18 L-8 30 Z", tx=96, ty=100, fill=t.INK)
    rays = []
    for i in range(8):
        a = math.radians(i * 45 + 22.5)
        x, y = 96 + math.cos(a) * 82, 96 + math.sin(a) * 82
        rays.append([s.light(f"ray{i}", round(x, 1), round(y, 1), 5)])
    s.sequence("burst", rays, step=0.12, hold=1.6, cycle=2.4, low=0.0)
    breathe(s, "breathe", "halo", scale=1.08, period=2.4)
    s.finish("Ein leuchtender Kreis mit Haken, um den Lichter aufblitzen: richtig erinnert.")


def nochmal():
    s = Scene("nochmal", 192, 192, "Nochmal")
    outer = s.add("circle", "o", x=96, y=96, radius=64)
    inner = s.add("circle", "i", x=96, y=96, radius=46)
    ring = s.cli("arrange", "boolean", s.file, "--op", "difference", "--nodes", f"{outer},{inner}")["id"]
    gap = s.add("rect", "gap", x=96, y=20, width=80, height=48)
    ring = s.cli("arrange", "boolean", s.file, "--op", "difference", "--nodes", f"{ring},{gap}")["id"]
    s.update(ring, fill=INDIGO)
    s.crescent(ring, -8, -8, INDIGO_SH)
    s.crescent(ring, 6, 6, INDIGO_HI)
    s.add("path", "head", d="M0 -22 L30 0 L0 22 Z", tx=93, ty=41, fill=INDIGO)
    s.glow("glow", 96, 96, 40, strength=0.6)
    s.add("circle", "core", x=96, y=96, radius=16, fill=INDIGO_HI)
    s.add("circle", "spark", x=90, y=90, radius=5, fill=CREAM, opacity=0.9)
    s.pulse("core", ["glow"], low=0.25, period=3.0)
    s.finish("Ein ruhiger Pfeil im Kreis um einen hellen Punkt: das kommt bald wieder.")


def geschafft():
    s = Scene("geschafft", 192, 192, "Geschafft")
    s.glow("halo", 96, 84, 70, strength=0.6)
    star = s.add("star", "star", x=96, y=78, radius=50, tips=5, inner=0.45, fill=AMBER, pivot="center")
    s.crescent(star, -8, -8, AMBER_SH)
    s.crescent(star, 6, 6, AMBER_HI)
    groups = []
    for i, x in enumerate((60, 96, 132)):
        s.glow(f"g{i}", x, 158, 16)
        s.light(f"l{i}", x, 158, 8)
        groups.append([f"g{i}", f"l{i}"])
    s.sequence("loci", groups, step=0.45, hold=2.4, cycle=3.0, low=0.2)
    breathe(s, "breathe", "star", scale=1.06, period=2.4)
    s.finish("Ein Stern über drei Loci, die nacheinander aufleuchten: Route geschafft.")


# ---------------------------------------------------------------- room kinds (48 x 48)

def raum_leer():
    s = Scene("raum-leer", 48, 48, "Raum ohne Bild")
    frame = s.add("path", "frame", d=rounded(36, 30, 6), tx=6, ty=9, fill=INDIGO)
    s.crescent(frame, -3, -3, INDIGO_SH)
    s.add("path", "inside", d=rounded(26, 20, 3), tx=11, ty=14, fill=t.INDIGO_DEEP)
    s.glow("g", 24, 24, 8)
    s.light("l", 24, 24, 3.5)
    s.pulse("l", ["g"], low=0.4, period=2.6)
    s.finish("Ein leerer Rahmen mit einem Locus: Raum ohne Bild.")


def raum_bild():
    s = Scene("raum-bild", 48, 48, "Raum mit Bild")
    frame = s.add("path", "frame", d=rounded(36, 30, 6), tx=6, ty=9, fill=INDIGO)
    s.crescent(frame, -3, -3, INDIGO_SH)
    s.add("path", "sky", d=rounded(26, 20, 3), tx=11, ty=14, fill=INDIGO_HI)
    s.add("path", "hill", d="M0 20 L0 12 C 6 6 12 8 16 12 C 19 9 23 9 26 12 L26 20 Z", tx=11, ty=14, fill=t.GREEN)
    s.add("circle", "sun", x=31, y=19, radius=3.5, fill=AMBER)
    s.glow("g", 18, 27, 7)
    s.light("l", 18, 27, 3)
    s.pulse("l", ["g"], low=0.4, period=2.6)
    s.finish("Ein Bild mit Hügel und Sonne und einem Locus: Raum mit Bild.")


def raum_video():
    s = Scene("raum-video", 48, 48, "Raum mit Video")
    frame = s.add("path", "frame", d=rounded(36, 30, 6), tx=6, ty=9, fill=INDIGO)
    s.crescent(frame, -3, -3, INDIGO_SH)
    s.add("path", "screen", d=rounded(26, 20, 3), tx=11, ty=14, fill=t.INDIGO_DEEP)
    s.add("path", "play", d="M0 -6 L10 0 L0 6 Z", tx=21, ty=24, fill=CREAM, pivot="center")
    s.glow("g", 34, 33, 6)
    s.light("l", 34, 33, 2.5)
    breathe(s, "play_anim", "play", scale=1.12, period=2.4)
    s.finish("Ein Bildschirm mit Abspielzeichen: Raum mit Video.")


if __name__ == "__main__":
    os.makedirs(t.OUT, exist_ok=True)
    for build in (kachel_core, kachel_review, kachel_notify, kachel_tutorial, kachel_gamification,
                  kachel_plugin, richtig, nochmal, geschafft, raum_leer, raum_bild, raum_video):
        build()
