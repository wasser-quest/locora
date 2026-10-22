#!/usr/bin/env python3
"""Builds Bruno, Simo's friend who explains palaces, places and routes (lineless, cel-shaded).

Usage: SPLINELY_CLI=splinely-cli python3 assets/mascot/src/bruno.py assets/mascot/bruno.sply
Bruno is named after Giordano Bruno, who wrote books on the art of memory. A tall, broad
man with a full ginger beard, a flat cap and a forest-green jacket; he holds a map with a
route of three glowing loci. Signature move: every few seconds he lifts his cap.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from figur import Figure, SIMO_FACE, JACKET, ellipse, tf, mx, same  # noqa: E402

P = dict(skin="#E8B892", skin_sh="#CC9670", skin_deep="#AE7653", skin_hi="#F5CDAD",
         hair="#B4572A", hair_sh="#8A3E1A", hair_hi="#D2733F", feature="#2E1A10")
GREEN, GREEN_SH, GREEN_HI = "#3F7D4A", "#2A5A33", "#5A9E62"
CAP, CAP_SH, CAP_HI = "#2F5F38", "#1E4226", "#467C4F"
BROWN, BROWN_SH = "#6B4A2E", "#4C331E"
MAP, MAP_SH = "#F2E2B8", "#D8C38E"
CREAM, CREAM_SH = "#F3EEE3", "#D3CBB9"

fig = Figure(sys.argv[1] if len(sys.argv) > 1 else "bruno.sply", "Bruno", P, hy=246)
y = fig.y

# ---------------------------------------------------------------- legs, jacket
fig.legs(556, BROWN, BROWN_SH, "#5A3A24", "#3E2716", None, width=58, spread=32)
neck = fig.neck(width=34)
fig.torso(tf(JACKET, 1.14, 1.0), y(376), GREEN, GREEN_SH, GREEN_HI)
fig.add("path", "shirt", d="M-34 0 L0 50 L34 0 Z", tx=256, ty=y(378), fill=CREAM)
flap = "M-4 0 C -18 2 -32 8 -42 16 C -36 22 -32 28 -29 36 C -20 32 -12 30 -4 30 Z"
fig.add("path", "collar_l", d=flap, tx=250, ty=y(376), fill=GREEN_HI)
fig.add("path", "collar_r", d=mx(flap), tx=262, ty=y(376), fill=GREEN_SH)
for i, by in enumerate((444, 488, 532)):
    fig.add("circle", f"button{i}", x=256, y=y(by), radius=6, fill=BROWN)
for side, x in (("l", 196), ("r", 316)):
    fig.add("path", f"pocket_{side}", d="M-26 0 L26 0 L26 5 L-26 5 Z", tx=x, ty=y(496), fill=GREEN_SH)

# ---------------------------------------------------------------- arms
for s in "lr":
    at = fig.arm(s, 120, GREEN, GREEN_SH, cuff=GREEN_SH, width=52)
# map (bone-local: origin = right hand): an unfolded sheet with a route of glowing loci
mp = [fig.add("path", "map_sheet", d="M-44 4 L-14 -2 L14 4 L44 -2 L44 70 L14 76 L-14 70 L-44 76 Z", fill=MAP)]
mp.append(fig.add("path", "map_fold1", d="M-14 -2 L14 4 L14 76 L-14 70 Z", fill=MAP_SH))
mp.append(fig.add("path", "map_route", d="M-30 60 Q -20 30 0 40 Q 20 50 30 14 L33 16 Q 22 55 0 44 Q -18 36 -27 61 Z",
                  fill="#C77B4A", opacity=0.8))
for i, (x, yy) in enumerate([(-28, 60), (0, 42), (30, 14)]):
    mp += fig.glow(f"map_locus{i}", x, yy, r=5, halo=12)
for s in "lr":
    fig.hand(s, at)

# ---------------------------------------------------------------- head
fig.ears(y=282)
face = fig.face(SIMO_FACE, neck)
# short hair at the sides under the cap, running into the beard
side_hair = []
for side, x in (("l", 170), ("r", 342)):
    side_hair.append(fig.add("path", f"sideburn_{side}", d="M-10 -40 L12 -40 L12 50 L-10 50 Z", tx=x, ty=y(250)))
sides = fig.boolean("union", side_hair)
window = fig.dup(face)
sides = fig.boolean("intersection", [sides, window])
fig.update(sides, fill=P["hair"])
fig.to_front([sides])
fig.eyes("#1F3A1C #3F7A3A #A8C978", "#0E1A0C", y=280, brow="M-26 5 L-22 -6 L24 -7 L26 3 Z")
fig.add("path", "nose", d="M3 -10 L-7 7 L3 10 L5 7 L-2 6 L6 -9 Z", tx=257, ty=y(308), fill=P["skin_deep"])
# full beard: the lower face plus a rounded chin, with a moustache over the mouth
region = [fig.add("path", "beard_area",
                  d="M-112 -60 L-82 -60 C -82 -10 -70 22 -40 28 C -20 32 20 32 40 28 C 70 22 82 -10 82 -60 L112 -60 L112 160 L-112 160 Z",
                  tx=256, ty=y(296))]
f2 = fig.dup(face)
beard = fig.boolean("intersection", [f2, region[0]])
chin = fig.add("path", "beard_chin", d=ellipse(64, 36), tx=256, ty=y(364))
beard = fig.boolean("union", [beard, chin])
fig.update(beard, fill=P["hair"])
fig.to_front([beard])
fig.crescent(beard, -14, -10, P["hair_sh"])
fig.crescent(beard, 8, 8, P["hair_hi"])
fig.add("path", "mouth", d="M-16 0 Q 0 12 16 -2 L16 2 Q 0 18 -16 4 Z", tx=256, ty=y(342), fill=P["feature"])
fig.add("path", "lip", d="M-10 9 Q 0 14 10 8 Q 0 20 -10 9 Z", tx=256, ty=y(342), fill="#C46F55")
fig.add("path", "moustache", d="M-38 8 C -28 -8 -8 -8 0 0 C 8 -8 28 -8 38 8 C 26 2 12 6 0 8 C -12 6 -26 2 -38 8 Z",
        tx=256, ty=y(330), fill=P["hair_sh"])
# short hair on top, seen when he lifts the cap
top = fig.add("path", "hair_top", d=ellipse(98, 66), tx=256, ty=y(196))
win = fig.add("path", "hair_window", d="M-90 0 C -50 -26 50 -26 90 0 L90 120 L-90 120 Z", tx=256, ty=y(196))
top = fig.boolean("difference", [top, win])
fig.update(top, fill=P["hair"])
fig.crescent(top, -12, -10, P["hair_sh"])
# the flat cap
cap_parts = [fig.add("path", "cap_crown", d="M-118 30 C -118 -40 -60 -70 0 -70 C 70 -70 124 -40 124 22 C 80 34 -80 36 -118 30 Z",
                     tx=256, ty=y(196), fill=CAP)]
cap_parts.append(fig.crescent("cap_crown", -14, -12, CAP_SH))
cap_parts.append(fig.crescent("cap_crown", 10, 10, CAP_HI))
cap_parts.append(fig.add("path", "cap_brim", d="M-104 0 C -60 -14 60 -14 104 0 C 110 10 100 22 90 22 C 50 12 -50 12 -90 22 C -100 22 -110 10 -104 0 Z",
                         tx=256, ty=y(214), fill=CAP_SH))
cap_parts.append(fig.add("path", "cap_button", d=ellipse(10, 6), tx=256, ty=y(128), fill=CAP_SH))
brim_shadow = fig.cast(face, "cap_brim", 2, 12, P["skin_sh"])

# ---------------------------------------------------------------- rig
fig.rig({"l": (128, y(406), 11), "r": (384, y(406), -11)},
        extra=[("map", "arm_r", -1, at, 11, mp), ("cap", "torso", 255, y(214), 0, cap_parts, (-256, -y(214)))])

# ---------------------------------------------------------------- animation
Y0 = y(214)
steps = [
    # lifts his cap in greeting: up and tilted, then back
    f"keyframe --clip greet --target cap --property y --time 5.0 --value {Y0} --autoplay --loop",
    f"keyframe --clip greet --target cap --property y --time 5.4 --value {Y0 - 18} --easing ease-out",
    f"keyframe --clip greet --target cap --property y --time 6.0 --value {Y0 - 18}",
    f"keyframe --clip greet --target cap --property y --time 6.4 --value {Y0} --easing ease-in",
    f"keyframe --clip greet --target cap --property y --time 7.0 --value {Y0}",
    "keyframe --clip greet --target cap --property rotation --time 5.0 --value 0",
    "keyframe --clip greet --target cap --property rotation --time 5.4 --value -10 --easing ease-out",
    "keyframe --clip greet --target cap --property rotation --time 6.0 --value -10",
    "keyframe --clip greet --target cap --property rotation --time 6.4 --value 0 --easing ease-in",
    f"keyframe --clip greet --target {brim_shadow} --property opacity --time 5.2 --value 1",
    f"keyframe --clip greet --target {brim_shadow} --property opacity --time 5.4 --value 0",
    f"keyframe --clip greet --target {brim_shadow} --property opacity --time 6.2 --value 0",
    f"keyframe --clip greet --target {brim_shadow} --property opacity --time 6.4 --value 1",
]
for i in range(3):
    t_on = round(0.35 + i * 0.5, 2)
    for n, lo in ((f"map_locus{i}_glow", 0.15), (f"map_locus{i}", 0.55)):
        extra = " --autoplay --loop" if (i == 0 and n.endswith("_glow")) else ""
        steps += [f"keyframe --clip loci --target {n} --property opacity --time 0.01 --value {lo}{extra}",
                  f"keyframe --clip loci --target {n} --property opacity --time {t_on} --value 1 --easing ease-out",
                  f"keyframe --clip loci --target {n} --property opacity --time 2.4 --value 1",
                  f"keyframe --clip loci --target {n} --property opacity --time 3 --value {lo} --easing ease-in"]
fig.animate(steps, "Bruno",
            "Bruno, ein Freund von Simo: ein großer, kräftiger Mann mit rotem Vollbart, Schiebermütze und "
            "waldgrüner Jacke, mit einer Karte, auf der drei Orte nacheinander leuchten. Er lüftet ab und zu die Mütze.",
            "Bruno")
