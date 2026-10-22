#!/usr/bin/env python3
"""Builds Cami, Simo's friend who explains how to picture things (lineless, cel-shaded).

Usage: SPLINELY_CLI=splinely-cli python3 assets/mascot/src/cami.py assets/mascot/cami.sply
Cami is named after Giulio Camillo, who built a wooden "theatre of memory" full of
pictures. A girl with dark brown skin and two braids that stick out sideways, in a
sunflower-yellow dress; she holds a big pencil with a glowing tip. Signature move:
every few seconds she twirls the pencil.
"""
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from figur import Figure, ROUND_FACE, JACKET, ellipse, tf, mx, same, LIME  # noqa: E402

P = dict(skin="#8D5A3B", skin_sh="#704229", skin_deep="#55301C", skin_hi="#A87150",
         hair="#1E1411", hair_sh="#0F0907", hair_hi="#3E2C25", feature="#1A0D07")
SUN, SUN_SH, SUN_HI = "#F2B230", "#CF8C12", "#FFD36B"
DENIM, DENIM_SH = "#2F4E86", "#1F3561"
CORAL, CORAL_SH = "#E8604C", "#B8402F"
CREAM, CREAM_SH = "#F3EEE3", "#D3CBB9"

fig = Figure(sys.argv[1] if len(sys.argv) > 1 else "cami.sply", "Cami", P, hy=330)
y = fig.y

# ---------------------------------------------------------------- legs, dress
fig.legs(598, DENIM, DENIM_SH, CREAM, CREAM_SH, CORAL, width=44, spread=22)
neck = fig.neck(width=24)
fig.torso(tf(JACKET, 0.8, 0.84), y(372), SUN, SUN_SH, SUN_HI)
fig.add("path", "collar", d="M-34 0 C -20 14 20 14 34 0 L30 -4 C 18 6 -18 6 -30 -4 Z", tx=256, ty=y(378), fill=SUN_SH)
pocket = fig.add("path", "pocket", d="M-26 0 L26 0 L24 30 C 10 36 -10 36 -24 30 Z", tx=256, ty=y(470), fill=SUN_SH)
heart = fig.glow("pocket_light", 256, y(482), r=5.5, halo=13)

# ---------------------------------------------------------------- arms
for s in "lr":
    at = fig.arm(s, 92, SUN, SUN_SH, cuff=CORAL, width=40)
# pencil (bone-local: origin = right hand), centred on the hand so it can twirl
pencil = [
    fig.add("circle", "pencil_glow", x=0, y=-64, radius=20,
            gradient="radial rgba(230,255,120,0.85)@0 rgba(230,255,120,0)@1", pivot="center"),
    fig.add("path", "pencil_body", d="M-9 -38 L9 -38 L9 50 L-9 50 Z", fill=CORAL),
]
pencil.append(fig.crescent("pencil_body", -6, 0, CORAL_SH))
pencil += [
    fig.add("path", "pencil_wood", d="M-9 -38 L9 -38 L0 -58 Z", fill="#F1D3A2"),
    fig.add("path", "pencil_lead", d="M-3.5 -50 L3.5 -50 L0 -59 Z", fill="#C9E85A"),
    fig.add("path", "pencil_band", d="M-9 50 L9 50 L9 56 L-9 56 Z", fill="#C9C9D6"),
    fig.add("path", "pencil_rubber", d="M-9 56 L9 56 L9 62 C 9 68 -9 68 -9 62 Z", fill="#F49AA0"),
]
for s in "lr":
    fig.hand(s, at)

# ---------------------------------------------------------------- head
# braids stick out sideways (behind the face): three plaits each and a lime bobble
braids = []
for side, sgn in (("l", -1), ("r", 1)):
    for i in range(3):
        cx, cy = 256 + sgn * (108 + i * 28), y(266 + i * 24)
        braids.append(fig.add("path", f"braid_{side}{i}", d=ellipse(23, 18), tx=cx, ty=cy, rotation=sgn * 36,
                              fill=P["hair"]))
        braids.append(fig.add("path", f"braid_hi_{side}{i}", d="M-12 -4 Q -2 -12 10 -8 L8 -5 Q -2 -8 -10 -1 Z",
                              tx=cx, ty=cy, rotation=sgn * 36, fill=P["hair_hi"]))
    bx, by = 256 + sgn * 200, y(340)
    braids.append(fig.add("path", f"tuft_{side}", d=ellipse(12, 16), tx=bx + sgn * 8, ty=by + 10, rotation=sgn * 36,
                          fill=P["hair"]))
    braids += fig.glow(f"bobble_{side}", bx, by, r=9, halo=18)
fig.ears(xs=(164, 348), y=286)
face = fig.face(ROUND_FACE, neck)
hair_parts = [fig.add("circle", "hair_core", x=256, y=y(214), radius=104)]
for side, x in (("l", 162), ("r", 350)):
    hair_parts.append(fig.add("circle", f"temple_{side}", x=x, y=y(248), radius=26))
hair_all = fig.boolean("union", hair_parts)
window = fig.add("path", "hairline",
                 d="M-82 -30 C -50 -42 -10 -40 0 -32 C 10 -40 50 -42 82 -30 C 88 -22 90 -12 90 0 L 90 220 L -90 220 L -90 0 C -90 -12 -88 -22 -82 -30 Z",
                 tx=256, ty=y(250))
hair = fig.boolean("difference", [hair_all, window])
fig.update(hair, fill=P["hair"])
fig.cast(face, hair, 4, 12, P["skin_sh"])
fig.eyes("#2A1206 #6B3A16 #B9813E", "#120602", y=290, brow="M-22 4 L-18 -5 L20 -6 L22 3 Z")
fig.nose_mouth(mouth="M-21 -4 Q 0 24 21 -4 Q 0 3 -21 -4 Z", nose_y=314, mouth_y=340)
fig.add("path", "tongue", d="M-8 10 Q 0 5 8 10 Q 0 16 -8 10 Z", tx=256, ty=fig.y(340), fill="#C9625A")
hair_sh = fig.crescent(hair, -16, -14, P["hair_sh"])
hair_hi = fig.crescent(hair, 10, 12, P["hair_hi"])
part = fig.add("path", "parting", d="M-2 0 L2 0 L3 46 L-3 46 Z", tx=256, ty=y(112), fill=P["skin_sh"])
fig.to_front([hair, hair_sh, hair_hi, part])

# ---------------------------------------------------------------- rig
fig.rig({"l": (166, y(400), 11), "r": (346, y(400), -11)},
        extra=[("pencil", "arm_r", -1, at, 11, pencil)])

# ---------------------------------------------------------------- animation
steps = [
    # twirl: one full turn of the pencil every 6 s, with a small swing of the arm
    "keyframe --clip twirl --target pencil --property rotation --time 4.8 --value 11 --autoplay --loop",
    "keyframe --clip twirl --target pencil --property rotation --time 5.7 --value 371 --easing ease-in-out",
    "keyframe --clip twirl --target pencil --property rotation --time 6 --value 371",
    "keyframe --clip twirl --target arm_r --property rotation --time 4.6 --value -11",
    "keyframe --clip twirl --target arm_r --property rotation --time 5.0 --value -24 --easing ease-out",
    "keyframe --clip twirl --target arm_r --property rotation --time 5.8 --value -11 --easing ease-in-out",
    "keyframe --clip twirl --target arm_r --property rotation --time 6 --value -11",
    "keyframe --clip glow --target pencil_glow --property opacity --time 1.2 --value 0.45 --easing ease-in-out --autoplay --loop",
    "keyframe --clip glow --target pencil_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip glow --target bobble_l_glow --property opacity --time 1.2 --value 0.4 --easing ease-in-out",
    "keyframe --clip glow --target bobble_l_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip glow --target bobble_r_glow --property opacity --time 1.2 --value 0.4 --easing ease-in-out",
    "keyframe --clip glow --target bobble_r_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip glow --target pocket_light_glow --property opacity --time 1.2 --value 0.4 --easing ease-in-out",
    "keyframe --clip glow --target pocket_light_glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
]
fig.animate(steps, "Cami",
            "Cami, eine Freundin von Simo: ein Mädchen mit dunkelbrauner Haut und zwei seitlich abstehenden Zöpfen "
            "im sonnengelben Kleid, mit einem großen Stift mit leuchtender Spitze, den sie ab und zu wirbeln lässt.",
            "Cami")
