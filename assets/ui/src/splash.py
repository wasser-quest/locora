#!/usr/bin/env python3
"""Builds the start-up splash animations with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/ui/src/splash.py [OUTPUT_DIR]

Four scenes, one of them is picked at random each time the app starts. They bounce
and hop like the method of loci: loci that jump along a route, a firefly that
bobs, a palace arch with hopping loci, a ball that bounces with squash and stretch.
No text: the tips are in the app's own language (apps/app/ui/index.html).
Style: assets/STIL.md (no outlines, cel shading, light from the top left).
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


def hop(s, clip, target, delay, height=44, duration=2.4):
    """One bounce per loop: up at `delay`, back down 0.36 s later, eased both ways."""
    s.steps += [
        f"clip add --id {clip} --duration {duration:g}",
        f"keyframe --clip {clip} --target {target} --property y --time {delay:g} --by -{height} --easing ease-out",
        f"keyframe --clip {clip} --target {target} --property y --time {delay + 0.36:g} --value 0 --easing ease-in",
        f"keyframe --clip {clip} --target {target} --property y --time {duration - 0.01:g} --value 0",
        f'on --event click --target {target} --play {clip}',
    ]


def loop_autoplay(s, clip):
    s.steps.append(f"clip update --id {clip} --autoplay --loop")


def route():
    s = Scene("splash-route", SIZE, SIZE, "Route")
    spots = [(70, 210), (160, 150), (250, 210)]
    for i, (x, y) in enumerate(spots):
        s.glow(f"g{i}", x, y, 38)
        s.add("circle", f"o{i}", x=x, y=y, radius=21, pivot="center", fill=INDIGO)
        s.crescent(f"o{i}", -4, -4, INDIGO_SH)
        s.light(f"l{i}", x, y, 6)
    for i in range(3):
        hop(s, f"route{i}", f"o{i}", delay=0.2 + i * 0.22)
        s.steps.append(f"clip update --id route{i} --autoplay --loop")
    s.finish("Drei Loci hüpfen nacheinander die Route entlang.")


def firefly():
    s = Scene("splash-firefly", SIZE, SIZE, "Glühwürmchen")
    s.glow("halo", 160, 150, 70, strength=0.7)
    body = s.add("circle", "body", x=160, y=150, radius=24, pivot="center", fill=INDIGO)
    s.crescent(body, -5, -5, INDIGO_SH)
    s.light("core", 160, 150, 9)
    s.steps += [
        "clip add --id bob --duration 2.6",
        "keyframe --clip bob --target body --property y --time 1.3 --by -26 --easing ease-in-out",
        "keyframe --clip bob --target body --property y --time 2.6 --value 0 --easing ease-in-out",
        "keyframe --clip bob --target halo --property y --time 1.3 --by -26 --easing ease-in-out",
        "keyframe --clip bob --target halo --property y --time 2.6 --value 0 --easing ease-in-out",
        "keyframe --clip bob --target core --property y --time 1.3 --by -26 --easing ease-in-out",
        "keyframe --clip bob --target core --property y --time 2.6 --value 0 --easing ease-in-out",
        "clip update --id bob --autoplay --loop",
    ]
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
        hop(s, f"arch{i}", f"o{i}", delay=0.15 + i * 0.25, height=30)
        s.steps.append(f"clip update --id arch{i} --autoplay --loop")
    s.finish("Ein Palast mit drei Loci, die im Eingang hüpfen.")


def ball():
    s = Scene("splash-ball", SIZE, SIZE, "Ball")
    s.add("path", "shadow", d=rounded(112, 18, 9), tx=104, ty=253, fill=t.INDIGO_DEEP, opacity=0.35)
    s.glow("halo", 160, 170, 56, strength=0.5)
    ball = s.add("circle", "ball", x=160, y=170, radius=34, pivot="center", fill=AMBER)
    s.crescent(ball, -7, -7, t.AMBER_SH if hasattr(t, "AMBER_SH") else "#B98A3A")
    s.light("shine", 150, 158, 7)
    s.steps += [
        "clip add --id bounce --duration 1.6",
        "keyframe --clip bounce --target ball --property y --time 0.5 --by -120 --easing ease-out",
        "keyframe --clip bounce --target ball --property y --time 0.8 --value 0 --easing ease-in",
        "keyframe --clip bounce --target ball --property scaleY --time 0.8 --value 0.8 --easing ease-out",
        "keyframe --clip bounce --target ball --property scaleY --time 0.95 --value 1 --easing ease-in-out",
        "keyframe --clip bounce --target ball --property scaleY --time 1.6 --value 1",
        "keyframe --clip bounce --target ball --property y --time 1.59 --value 0",
        "clip update --id bounce --autoplay --loop",
    ]
    s.finish("Ein Ball hüpft mit Quetschen und Strecken, wie ein Gummiball.")


SCENES = [route, firefly, arch, ball]

if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for build in SCENES:
        build()
