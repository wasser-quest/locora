#!/usr/bin/env python3
"""Builds Hafiza, Simo's friend who explains numbers, repetition and memory sports (lineless, cel-shaded).

Usage: SPLINELY_CLI=splinely-cli python3 assets/mascot/src/hafiza.py assets/mascot/hafiza.sply
Hafiza's name means "memory" (Turkish hafıza, from Arabic). A young woman with warm brown
skin in a raspberry headscarf and a long coat; she holds a stopwatch with a glowing face.
Signature move: every few seconds she lifts the stopwatch and checks the time.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from figur import Figure, SIMO_FACE, JACKET, ellipse, tf, mx, same  # noqa: E402

P = dict(skin="#B98260", skin_sh="#9A6646", skin_deep="#7E4F33", skin_hi="#CF9C7C",
         hair="#2B1A12", hair_sh="#2B1A12", hair_hi="#2B1A12", feature="#24130B")
SCARF, SCARF_SH, SCARF_HI = "#D2487A", "#A3305A", "#E86F99"
COAT, COAT_SH, COAT_HI = "#8E2F52", "#6B1F3C", "#AE4670"
SLATE, SLATE_SH = "#3D3B57", "#2A283E"
SILVER, SILVER_SH = "#D9DDE6", "#A8AEBD"
CREAM, CREAM_SH = "#F3EEE3", "#D3CBB9"

fig = Figure(sys.argv[1] if len(sys.argv) > 1 else "hafiza.sply", "Hafiza", P, hy=262)
y = fig.y

# ---------------------------------------------------------------- legs, coat
fig.legs(612, SLATE, SLATE_SH, CREAM, CREAM_SH, SCARF, width=46, spread=24)
coat = "M-70 0 C -102 4 -122 20 -124 52 L-126 236 C -70 248 70 248 126 236 L124 52 C 122 20 102 4 70 0 Z"
fig.torso(coat, y(376), COAT, COAT_SH, COAT_HI)
fig.add("path", "coat_split", d="M-2 0 L2 0 L2 70 L-2 70 Z", tx=256, ty=y(552), fill=COAT_SH)
for i, by in enumerate((456, 500, 544)):
    fig.add("circle", f"button{i}", x=256, y=y(by), radius=6, fill=COAT_SH)
fig.add("path", "belt", d="M-118 0 L118 0 L118 12 L-118 12 Z", tx=256, ty=y(512), fill=COAT_SH)

# ---------------------------------------------------------------- arms
for s in "lr":
    at = fig.arm(s, 112, COAT, COAT_SH, cuff=SCARF)
# stopwatch (bone-local: origin = right hand)
watch = [
    fig.add("path", "watch_crown", d="M-6 0 L6 0 L6 10 L-6 10 Z", ty=-6, fill=SILVER_SH),
    fig.add("path", "watch_ring", d="M-8 0 C -8 -12 8 -12 8 0 L4 0 C 4 -7 -4 -7 -4 0 Z", ty=-6, fill=SILVER_SH),
    fig.add("circle", "watch_case", x=0, y=30, radius=26, fill=SILVER),
]
watch.append(fig.crescent("watch_case", -7, -7, SILVER_SH))
watch += [
    fig.add("circle", "watch_glow", x=0, y=30, radius=34,
            gradient="radial rgba(230,255,120,0.75)@0 rgba(230,255,120,0)@1", pivot="center"),
    fig.add("circle", "watch_face", x=0, y=30, radius=19, gradient="radial #FFFFF0@0 #F3FF9A@0.6 #C9E85A@1"),
    fig.add("path", "watch_hand", d="M-2 2 L-1.5 -15 L1.5 -15 L2 2 Z", ty=30, fill="#2A283E"),
    fig.add("circle", "watch_pin", x=0, y=30, radius=3, fill="#2A283E"),
]
for s in "lr":
    fig.hand(s, at)

# ---------------------------------------------------------------- head
face = fig.face(SIMO_FACE)
fig.eyes("#2E1608 #7A4A1E #C99A55", "#160903", brow="M-24 4 L-20 -5 L22 -6 L24 2 Z", brow_fill="#2B1A12")
fig.nose_mouth()
# headscarf: the head and a drape over the shoulders, with the face left free
outer = [fig.add("circle", "scarf_head", x=256, y=y(230), radius=122),
         fig.add("path", "scarf_drape", d="M-116 0 C -122 50 -128 92 -112 118 C -60 138 60 138 112 118 C 128 92 122 50 116 0 Z", tx=256, ty=y(290))]
scarf = fig.boolean("union", outer)
window = fig.add("path", "face_window", d=tf(SIMO_FACE, 0.93, 0.86), tx=256, ty=y(268))
scarf = fig.boolean("difference", [scarf, window])
fig.update(scarf, fill=SCARF)
fig.cast(face, scarf, 4, 12, P["skin_sh"])
scarf_sh = fig.crescent(scarf, -18, -14, SCARF_SH)
scarf_hi = fig.crescent(scarf, 10, 12, SCARF_HI)
pleats = [fig.add("path", "pleat_l", d="M0 0 C -8 26 -22 50 -46 70 L-40 74 C -16 54 -2 30 6 2 Z", tx=198, ty=y(352), fill=SCARF_SH),
          fig.add("path", "pleat_r", d="M0 0 C 8 26 22 50 46 70 L40 74 C 16 54 2 30 -6 2 Z", tx=314, ty=y(352), fill=SCARF_SH)]
pin = fig.glow("scarf_pin", 330, y(330), r=5.5, halo=13)
fig.to_front([scarf, scarf_sh, scarf_hi, *pleats, *pin])

# ---------------------------------------------------------------- rig
fig.rig({"l": (148, y(406), 11), "r": (364, y(406), -11)},
        extra=[("watch", "arm_r", -1, at, 11, watch)])

# ---------------------------------------------------------------- animation
steps = [
    # the second hand runs; every 6 s she lifts the stopwatch in front of her and checks it
    "keyframe --clip tick --target watch_hand --property rotation --time 6 --value 360 --autoplay --loop",
    "keyframe --clip check --target arm_r --property rotation --time 3.6 --value -11 --autoplay --loop",
    "keyframe --clip check --target arm_r --property rotation --time 4.2 --value 52 --easing ease-in-out",
    "keyframe --clip check --target arm_r --property rotation --time 5.4 --value 50",
    "keyframe --clip check --target arm_r --property rotation --time 6.0 --value -11 --easing ease-in-out",
    "keyframe --clip check --target watch --property rotation --time 3.6 --value 11",
    "keyframe --clip check --target watch --property rotation --time 4.2 --value -52 --easing ease-in-out",
    "keyframe --clip check --target watch --property rotation --time 5.4 --value -50",
    "keyframe --clip check --target watch --property rotation --time 6.0 --value 11 --easing ease-in-out",
    "keyframe --clip glow --target watch_glow --property opacity --time 1.2 --value 0.45 --easing ease-in-out --autoplay --loop",
    "keyframe --clip glow --target watch_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip glow --target scarf_pin_glow --property opacity --time 1.2 --value 0.4 --easing ease-in-out",
    "keyframe --clip glow --target scarf_pin_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
]
fig.animate(steps, "Hafiza",
            "Hafiza, eine Freundin von Simo: eine junge Frau mit warmem braunem Hautton, himbeerrotem Kopftuch und "
            "langem Mantel, mit einer Stoppuhr mit leuchtendem Zifferblatt, auf die sie ab und zu schaut.",
            "Hafiza")
