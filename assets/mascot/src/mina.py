#!/usr/bin/env python3
"""Builds Mina, Simo's friend who explains the basics and the research (lineless, cel-shaded).

Usage: SPLINELY_CLI=splinely-cli python3 assets/mascot/src/mina.py assets/mascot/mina.sply
Mina is named after Mnemosyne, the Greek goddess of memory. An older woman with fair
skin, silver hair in a tall bun, round glasses and a teal dress; she holds a book with
a glowing bookmark. Signature move: every few seconds she pushes her glasses back up.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from figur import Figure, ROUND_FACE, ellipse, mx, same, capsule, LIME  # noqa: E402

P = dict(skin="#F1CBB0", skin_sh="#D9A68A", skin_deep="#BE8268", skin_hi="#FADFCB",
         hair="#DCDCE6", hair_sh="#AEAEC2", hair_hi="#F6F6FB", feature="#3A2420")
TEAL, TEAL_SH, TEAL_HI = "#2A8C88", "#1B6562", "#43ABA6"
CREAM, CREAM_SH = "#F3EEE3", "#D3CBB9"
FRAME = "#3B2A33"
BOOK, BOOK_SH, PAGES = "#E07A5F", "#B85840", "#F7F1E3"

fig = Figure(sys.argv[1] if len(sys.argv) > 1 else "mina.sply", "Mina", P, hy=272)
y = fig.y

# ---------------------------------------------------------------- legs, dress
fig.legs(612, "#55505E", "#3C3846", "#7A3B4C", "#5A2836", None, width=42, spread=24)
neck = fig.neck(width=26)
dress = ("M-66 0 C -96 4 -116 20 -118 50 L-134 222 C -72 236 72 236 134 222 L118 50 C 116 20 96 4 66 0 Z")
fig.torso(dress, y(376), TEAL, TEAL_SH, TEAL_HI)
fig.add("path", "blouse", d="M-30 0 L0 40 L30 0 Z", tx=256, ty=y(378), fill=CREAM)
flap = "M-4 0 C -16 2 -28 7 -38 14 C -32 20 -26 26 -22 34 C -16 30 -10 28 -4 28 Z"
fig.add("path", "collar_l", d=flap, tx=251, ty=y(376), fill=CREAM)
fig.add("path", "collar_r", d=mx(flap), tx=261, ty=y(376), fill=CREAM_SH)
for i, by in enumerate((442, 482, 522)):
    fig.add("circle", f"button{i}", x=256, y=y(by), radius=5, fill=TEAL_SH)
brooch = fig.glow("brooch", 214, y(426), r=6, halo=15)

# ---------------------------------------------------------------- arms
for s in "lr":
    at = fig.arm(s, 110, TEAL, TEAL_SH, cuff=CREAM)
# book (bone-local: origin = hand of the right arm)
book = [
    fig.add("rect", "book_pages", x=-24, y=-2, width=56, height=80, rx=4, fill=PAGES),
    fig.add("rect", "book_cover", x=-32, y=-6, width=58, height=82, rx=6, fill=BOOK),
]
book.append(fig.crescent("book_cover", -8, -8, BOOK_SH))
book.append(fig.add("path", "book_band", d="M-32 14 L26 14 L26 22 L-32 22 Z", fill=TEAL))
book.append(fig.add("path", "book_ribbon", d="M2 74 L10 74 L10 96 L6 90 L2 96 Z", fill=LIME))
book.append(fig.add("circle", "book_glow", x=6, y=96, radius=16,
                    gradient="radial rgba(230,255,120,0.8)@0 rgba(230,255,120,0)@1", pivot="center"))
for s in "lr":
    fig.hand(s, at)

# ---------------------------------------------------------------- head
face = fig.face(ROUND_FACE, neck)
# hair: a soft bob around the face and a tall round bun on top
hair_parts = [
    fig.add("circle", "hair_core", x=256, y=y(206), radius=108),
    fig.add("path", "hair_bob", d=ellipse(110, 80), tx=256, ty=y(244)),
    fig.add("circle", "bun", x=256, y=y(78), radius=50),
    fig.add("path", "bun_neck", d="M-30 0 L30 0 L36 40 L-36 40 Z", tx=256, ty=y(100)),
]
hair_all = fig.boolean("union", hair_parts)
# swept fringe: the window is higher on the left, so the hair falls to the right
window = fig.add("path", "hairline",
                 d="M-80 -18 C -50 -44 10 -50 50 -38 C 70 -32 84 -24 88 -6 L 88 220 L -88 220 L -88 0 C -88 -8 -86 -14 -80 -18 Z",
                 tx=256, ty=y(240))
hair = fig.boolean("difference", [hair_all, window])
fig.update(hair, fill=P["hair"])
fig.cast(face, hair, 4, 12, P["skin_sh"])
fig.eyes("#2B3A4A #4F7393 #A9C7DD", "#141C26", brow="M-22 2 L-18 -4 L20 -5 L22 1 Z", brow_fill="#9C9CB0")
# smile lines at the outer eye corners (age, kindness)
for side, x, f in (("l", 214, mx), ("r", 298, same)):
    fig.add("path", f"smile_line_{side}", d=f("M30 4 L38 8 L37 10 L29 7 Z"), tx=x, ty=y(284), fill=P["skin_deep"])
fig.nose_mouth(mouth="M-16 -2 Q 0 13 16 -3 L17 0 Q 0 17 -16 1 Z")
hair_sh = fig.crescent(hair, -16, -14, P["hair_sh"])
hair_hi = fig.crescent(hair, 10, 12, P["hair_hi"])
band = fig.add("path", "bun_band", d="M-34 0 C -20 -6 20 -6 34 0 L34 10 C 20 4 -20 4 -34 10 Z", tx=256, ty=y(116), fill=TEAL)
strands = [fig.add("path", f"strand{i}", d="M-10 0 Q 0 -10 12 -4 L11 -1 Q 0 -6 -9 3 Z", tx=x, ty=y(yy),
                   rotation=r, fill=P["hair_sh"] if x > 256 else P["hair_hi"], opacity=0.9)
           for i, (x, yy, r) in enumerate([(232, 70, -20), (276, 60, 20), (186, 168, -30), (326, 166, 25),
                                           (220, 140, -10), (296, 140, 12)])]
fig.to_front([hair, hair_sh, hair_hi, band, *strands])

# glasses: round lenses as flat rings (shapes with thickness, not outlines)
glasses = []
for side, x in (("l", 214), ("r", 298)):
    outer = fig.add("path", f"rim_o_{side}", d=ellipse(31, 27), tx=x, ty=y(286))
    inner = fig.add("path", f"rim_i_{side}", d=ellipse(26, 22), tx=x, ty=y(286))
    fig.to_front([inner])
    rim = fig.boolean("difference", [outer, inner])
    fig.update(rim, fill=FRAME)
    glasses.append(rim)
    glasses.append(fig.add("path", f"lens_shine_{side}", d="M-16 -8 L-8 -16 L-4 -14 L-12 -6 Z", tx=x, ty=y(286),
                           fill="white", opacity=0.55))
glasses.append(fig.add("path", "bridge", d="M-12 -2 Q 0 -8 12 -2 L12 2 Q 0 -4 -12 2 Z", tx=256, ty=y(282), fill=FRAME))
for side, x, f in (("l", 183, mx), ("r", 329, same)):
    glasses.append(fig.add("path", f"temple_{side}", d=f("M0 -3 L16 -6 L16 -1 L0 2 Z"), tx=x, ty=y(282), fill=FRAME))

# the left arm reaches the glasses, so it is drawn in front of the face
fig.to_front(fig.arm_parts["l"])

# ---------------------------------------------------------------- rig
fig.rig({"l": (148, y(406), 11), "r": (364, y(406), -11)},
        extra=[("book", "arm_r", -1, at, 11, book), ("glasses", "torso", -1, 0, 0, glasses)])

# ---------------------------------------------------------------- animation
steps = []
# glasses slip down slowly, then the hand comes up and pushes them back
steps += [
    "keyframe --clip push --target glasses --property y --time 4.6 --value 4 --easing ease-in-out --autoplay --loop",
    "keyframe --clip push --target glasses --property y --time 5.3 --value 4",
    "keyframe --clip push --target glasses --property y --time 5.6 --value -1 --easing ease-out",
    "keyframe --clip push --target glasses --property y --time 7.2 --value 0 --easing ease-in-out",
    "keyframe --clip push --target arm_l --property rotation --time 4.6 --value 11",
    "keyframe --clip push --target arm_l --property rotation --time 5.3 --value -150 --easing ease-in-out",
    "keyframe --clip push --target arm_l --property rotation --time 5.7 --value -152",
    "keyframe --clip push --target arm_l --property rotation --time 6.4 --value 11 --easing ease-in-out",
    "keyframe --clip push --target arm_l --property rotation --time 7.2 --value 11",
    "keyframe --clip bookswing --target book --property rotation --time 1.6 --value 15 --easing ease-in-out --autoplay --loop",
    "keyframe --clip bookswing --target book --property rotation --time 3.2 --value 11 --easing ease-in-out",
    "keyframe --clip glow --target book_glow --property opacity --time 1.2 --value 0.45 --easing ease-in-out --autoplay --loop",
    "keyframe --clip glow --target book_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip glow --target brooch_glow --property opacity --time 1.2 --value 0.4 --easing ease-in-out",
    "keyframe --clip glow --target brooch_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
]
fig.animate(steps, "Mina",
            "Mina, eine Freundin von Simo: eine ältere Frau mit heller Haut, silbernem Haardutt, runder Brille und "
            "petrolfarbenem Kleid, mit einem Buch mit leuchtendem Lesezeichen. Sie schiebt ab und zu ihre Brille hoch.",
            "Mina")
