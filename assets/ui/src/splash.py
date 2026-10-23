#!/usr/bin/env python3
"""Builds the start-up splash animations with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/ui/src/splash.py [OUTPUT_DIR]

Four scenes, one of them is picked at random each time the app starts. They bounce
and hop like the method of loci: loci that jump along a route, a firefly that
bobs, a palace arch with hopping loci, a ball that bounces with squash and stretch.
Every moving thing is a `Body` (assets/tutorial/src/tutorial.py): its cel shadow,
shine and halo move with it, and the ball's ground shadow shrinks and fades as it rises.
No text: the tips are in the app's own language (apps/app/ui/index.html).
Style: assets/STIL.md (no outlines, cel shading, light from the top left).

Afterwards export the SVGs and inline them into the page:
  for n in route firefly arch ball; do
    splinely-cli export assets/ui/splash/splash-$n.sply --output apps/app/ui/splash/splash-$n.svg; done
  python3 assets/ui/src/splash_html.py
"""
import os, sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "tutorial", "src"))
import tutorial as t  # noqa: E402
from tutorial import Scene, rounded  # noqa: E402

OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "splash"))
t.OUT = OUT
INDIGO, INDIGO_SH, INDIGO_HI = t.INDIGO, t.INDIGO_SH, t.INDIGO_HI
CREAM, LIME, AMBER, GREEN = t.CREAM, t.LIME, t.AMBER, t.GREEN
SIZE = 320


def route():
    s = Scene("splash-route", SIZE, SIZE, "Route")
    spots = [(70, 210), (160, 150), (250, 210)]
    for i, (x, y) in enumerate(spots):
        s.glow(f"g{i}", x, y, 38)
        s.add("circle", f"o{i}", x=x, y=y, radius=21, pivot="center", fill=INDIGO)
        shade = s.crescent(f"o{i}", -4, -4, INDIGO_SH)
        s.light(f"l{i}", x, y, 6)
        # Halo, locus, its cel shadow and its light hop as one.
        locus = s.body(f"Locus {i + 1}", [f"g{i}", f"o{i}", shade, f"l{i}"], (x, y))
        locus.hop(f"route{i}", peak=0.2 + i * 0.22, height=44, duration=2.4)
        s.steps += [f"on --event click --target o{i} --play route{i}", f'a11y --node o{i} --label "Locus {i + 1}"']
    s.finish("Drei Loci hüpfen nacheinander die Route entlang.")


def firefly():
    s = Scene("splash-firefly", SIZE, SIZE, "Glühwürmchen")
    s.glow("halo", 160, 150, 70, strength=0.7)
    body = s.add("circle", "body", x=160, y=150, radius=24, pivot="center", fill=INDIGO)
    shade = s.crescent(body, -5, -5, INDIGO_SH)
    s.light("core", 160, 150, 9)
    fly = s.body("Glühwürmchen", ["halo", body, shade, "core"], (160, 150))
    fly.animate("bob", [(0, "ease-in-out", {"lift": 0}), (1.3, "ease-in-out", {"lift": 26}),
                        (2.6, "linear", {"lift": 0})])
    groups = []
    for i, x in enumerate((90, 160, 230)):
        s.glow(f"d{i}", x, 260, 16)
        s.light(f"e{i}", x, 260, 7)
        groups.append([f"d{i}", f"e{i}"])
    s.sequence("trail", groups, step=0.5, hold=2.4, cycle=3.0, low=0.25)
    s.finish("Ein Glühwürmchen bobbt auf und ab, drei Lichter leuchten nacheinander.")


def arch():
    s = Scene("splash-arch", SIZE, SIZE, "Palast")
    door = s.add("path", "arch", d="M-96 120 L-96 -20 C -96 -90 -60 -120 0 -120 C 60 -120 96 -90 96 -20 L96 120 Z",
                 tx=160, ty=230, fill=INDIGO)
    s.crescent(door, -8, -8, INDIGO_SH)
    s.add("path", "inside", d="M-60 120 L-60 0 C -60 -50 -36 -70 0 -70 C 36 -70 60 -50 60 0 L60 120 Z",
          tx=160, ty=230, fill=t.INDIGO_DEEP)
    spots = [(120, 190), (160, 150), (200, 190)]
    for i, (x, y) in enumerate(spots):
        s.glow(f"g{i}", x, y, 26)
        s.add("circle", f"o{i}", x=x, y=y, radius=11, pivot="center", fill=LIME)
        locus = s.body(f"Locus {i + 1}", [f"g{i}", f"o{i}"], (x, y))
        locus.hop(f"arch{i}", peak=0.15 + i * 0.25, height=30, duration=2.4)
        s.steps += [f"on --event click --target o{i} --play arch{i}", f'a11y --node o{i} --label "Locus {i + 1}"']
    s.finish("Ein Palast mit drei Loci, die im Eingang hüpfen.")


def ball():
    s = Scene("splash-ball", SIZE, SIZE, "Ball")
    s.add("path", "shadow", d=rounded(112, 18, 9), tx=104, ty=253, fill=t.INDIGO_DEEP, opacity=0.35)
    # The ball rests on its shadow (bottom at y 262) and squashes around that contact point.
    x, y, r = 160, 228, 34
    s.glow("halo", x, y, 56, strength=0.5)
    ball = s.add("circle", "ball", x=x, y=y, radius=r, pivot="center", fill=AMBER)
    shade = s.crescent(ball, -7, -7, "#B98A3A")
    s.light("shine", x - 10, y - 12, 7)
    rubber = s.body("Ball", ["halo", ball, shade, "shine"], (x, y + r))
    rubber.ground("shadow", opacity=0.35, highest=120)
    # Stretched while it falls, right down to the contact; it squashes only once it is down.
    rubber.animate("bounce", [
        (0, "ease-out", {"lift": 0, "sx": 1, "sy": 1}),             # take-off
        (0.5, "ease-in", {"lift": 120, "sx": 1, "sy": 1}),          # top, round
        (0.72, "linear", {"sx": 0.94, "sy": 1.08}),                 # falling fast: stretched
        (0.79, "ease-out", {"lift": 0, "sx": 0.94, "sy": 1.08}),    # contact, still stretched
        (0.84, "ease-in-out", {"lift": 0, "sx": 1.12, "sy": 0.8}),  # on the ground: squashed
        (0.97, "ease-in-out", {"lift": 0, "sx": 1, "sy": 1}),
        (1.6, "linear", {"lift": 0, "sx": 1, "sy": 1}),
    ])
    s.finish("Ein Ball hüpft mit Quetschen und Strecken, wie ein Gummiball.")


SCENES = [route, firefly, arch, ball]

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for build in SCENES:
        build()
