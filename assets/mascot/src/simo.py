#!/usr/bin/env python3
"""Builds Simo (lineless, cel-shaded, full body) with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/mascot/src/simo.py assets/mascot/simo.sply
Light comes from the top left. Every part is a flat base colour plus hard
shadow/light shapes made with boolean operations, so shadows follow the
silhouette exactly. There are no outlines.
"""
import json, math, os, re, subprocess, sys

CLI = os.environ.get("SPLINELY_CLI", "splinely-cli")
F = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "simo.sply")

SKIN, SKIN_SH, SKIN_DEEP, SKIN_HI = "#C88B5E", "#A86C44", "#8E5634", "#DDA77C"
HAIR, HAIR_SH, HAIR_HI = "#3E2416", "#24140B", "#5E3A24"
JACKET, JACKET_SH, JACKET_HI = "#3A3D92", "#25276A", "#5258BC"
PANTS, PANTS_SH = "#2A2C70", "#1A1B4C"
CREAM, CREAM_SH = "#F3EEE3", "#D3CBB9"
LIME, TRIM = "#DDF76A", "#C9E85A"
FEATURE = "#2A160C"  # eyes, brows, mouth (features, not outlines)

def cli(*args):
    out = subprocess.run([CLI, *args], capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"splinely-cli {' '.join(args)}\n{out.stdout}{out.stderr}")
    return json.loads(out.stdout) if out.stdout.strip() else {}

def add(kind, id, **o):
    args = ["node", "add", F, "--kind", kind, "--id", id]
    for k, v in o.items():
        args += [f"--{k.replace('_', '-')}", str(v)]
    cli(*args)
    return id

def update(id, **o):
    args = ["node", "update", F, id]
    for k, v in o.items():
        args += [f"--{k.replace('_', '-')}", str(v)]
    cli(*args)

def dup(id, dx=0, dy=0):
    return cli("node", "duplicate", F, "--nodes", id, "--dx", str(dx), "--dy", str(dy))["created"][0]

def boolean(op, ids):
    return cli("arrange", "boolean", F, "--op", op, "--nodes", ",".join(ids))["id"]

def crescent(base, dx, dy, fill, opacity=None):
    """The part of `base` NOT covered by a copy shifted by (dx, dy): a rim that follows the silhouette."""
    a, b = dup(base), dup(base, dx, dy)
    # duplicates sit right above their original; put the shifted copy on top
    # so the difference keeps `a` minus `b` (boolean subtracts from the bottom-most)
    cli("arrange", "order", F, "--nodes", b, "--to", "front")
    res = boolean("difference", [a, b])
    update(res, fill=fill, **({"opacity": opacity} if opacity is not None else {}))
    return res

def cast(target, caster, dx, dy, fill, opacity=None):
    """Shadow of `caster` shifted by (dx, dy), kept only where it falls on `target`."""
    a, b = dup(target), dup(caster, dx, dy)
    res = boolean("intersection", [a, b])
    update(res, fill=fill, **({"opacity": opacity} if opacity is not None else {}))
    return res

def ellipse(rx, ry):
    k = 0.5523
    return (f"M{-rx:g} 0 C {-rx:g} {-ry*k:.2f} {-rx*k:.2f} {-ry:g} 0 {-ry:g} C {rx*k:.2f} {-ry:g} {rx:g} {-ry*k:.2f} {rx:g} 0 "
            f"C {rx:g} {ry*k:.2f} {rx*k:.2f} {ry:g} 0 {ry:g} C {-rx*k:.2f} {ry:g} {-rx:g} {ry*k:.2f} {-rx:g} 0 Z")

def tf(d, sx=1, sy=1, dx=0, dy=0):
    res, ix = [], True
    for tok in re.findall(r"[A-Za-z]|-?\d*\.?\d+", d):
        if tok.isalpha():
            res.append(tok); ix = True; continue
        v = float(tok) * (sx if ix else sy) + (dx if ix else dy)
        res.append(f"{v:.2f}".rstrip("0").rstrip(".")); ix = not ix
    return " ".join(res)

mx = lambda d: tf(d, -1, 1)

def capsule(w, l):
    r = w / 2
    return f"M{-r:g} 0 C {-r:g} {-r*1.1:.1f} {r:g} {-r*1.1:.1f} {r:g} 0 L{r:g} {l:g} C {r:g} {l+r*1.1:.1f} {-r:g} {l+r*1.1:.1f} {-r:g} {l:g} Z"

if os.path.exists(F):
    os.remove(F)
cli("new", "--output", F, "--width", "512", "--height", "768", "--name", "Simo")

# ---------------------------------------------------------------- legs & shoes
for side, x, rot in (("l", 228, 2), ("r", 284, -2)):
    leg = add("path", f"leg_{side}", d=capsule(54, 112), tx=x, ty=560, rotation=rot, fill=PANTS)
    crescent(leg, -12, -6, PANTS_SH)
hips = add("path", "hips", d="M-74 0 L74 0 L64 46 L-64 46 Z", tx=256, ty=536, fill=PANTS)
crescent(hips, -14, -14, PANTS_SH)
shoe = "M-40 4 C -40 -18 20 -22 36 -6 C 44 4 40 18 24 18 L-30 18 C -40 18 -42 12 -40 4 Z"
for side, x, f in (("l", 220, mx), ("r", 292, lambda d: d)):
    s = add("path", f"shoe_{side}", d=f(shoe), tx=x, ty=688, fill=CREAM)
    crescent(s, -8, -8, CREAM_SH)
    add("path", f"sole_{side}", d=f("M-38 12 L36 12 C 40 12 38 18 24 18 L-30 18 C -40 18 -42 14 -38 12 Z"), tx=x, ty=688, fill=CREAM_SH)
    add("path", f"stripe_{side}", d=f("M-24 2 L18 2 L16 7 L-24 7 Z"), tx=x, ty=688, fill=TRIM)

# ---------------------------------------------------------------- neck & torso
neck = add("path", "neck", d="M-30 0 L-30 50 L30 50 L30 0 Z", tx=256, ty=342, fill=SKIN_SH)
jacket = add("path", "jacket", d="M-72 0 C -104 4 -126 20 -128 52 L-110 186 C -66 196 66 196 110 186 L128 52 C 126 20 104 4 72 0 Z", tx=256, ty=376, fill=JACKET)
crescent(jacket, -18, -12, JACKET_SH)
crescent(jacket, 10, 10, JACKET_HI)
shirt = add("path", "shirt", d="M-30 0 L0 54 L30 0 Z", tx=256, ty=378, fill=CREAM)
crescent(shirt, -8, 0, CREAM_SH)
flap = "M-4 0 C -16 2 -28 7 -38 14 C -32 19 -28 25 -25 33 C -18 29 -11 28 -4 29 Z"
add("path", "collar_l", d=flap, tx=251, ty=376, fill=JACKET_HI)
add("path", "collar_r", d=mx(flap), tx=261, ty=376, fill=JACKET_SH)
add("path", "zip", d="M-1.5 0 L1.5 0 L1.5 128 L-1.5 128 Z", tx=256, ty=432, fill=TRIM)
for side, x in (("l", 206), ("r", 306)):
    add("path", f"pocket_{side}", d="M-22 0 L22 0 L22 4 L-22 4 Z", tx=x, ty=498, fill=JACKET_SH)
for i, (x, y) in enumerate([(206, 418), (219, 434), (232, 450)]):
    add("circle", f"pin_glow{i}", x=x, y=y, radius=13, gradient="radial rgba(230,255,120,0.8)@0 rgba(230,255,120,0)@1", pivot="center")
    add("circle", f"pin{i}", x=x, y=y, radius=5.5, gradient=f"radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1", pivot="center")

# ---------------------------------------------------------------- arms (bone-local: origin = shoulder)
arm_parts = {"l": [], "r": []}
for side in "lr":
    sl = add("path", f"sleeve_{side}", d=capsule(46, 116), fill=JACKET)
    arm_parts[side] += [sl, crescent(sl, -12, -4, JACKET_SH)]
    arm_parts[side].append(add("path", f"cuff_{side}", d="M-24 0 L24 0 L24 9 L-24 9 Z", ty=104, fill=TRIM))
# lantern (bone-local: origin = hand)
add("circle", "lantern_halo", x=0, y=48, radius=60, gradient="radial rgba(236,255,140,0.75)@0 rgba(214,250,110,0.25)@0.5 rgba(200,240,90,0)@1", pivot="center")
add("path", "lantern_string", d="M-1.5 0 L1.5 0 L1.5 22 L-1.5 22 Z", fill=JACKET_SH)
add("path", "lantern_cap", d="M-16 0 L16 0 L12 10 L-12 10 Z", ty=22, fill=JACKET)
add("rect", "lantern_glass", x=-14, y=32, width=28, height=36, rx=10, gradient="radial #FFFFF0@0 #F3FF9A@0.5 #BEEB3C@1")
add("path", "lantern_bars", d="M-6 34 L-4 34 L-4 66 L-6 66 Z M4 34 L6 34 L6 66 L4 66 Z", fill="rgba(46,47,120,0.45)")
add("path", "lantern_base", d="M-12 0 L12 0 L16 8 L-16 8 Z", ty=68, fill=JACKET_SH)
for side in "lr":
    h = add("circle", f"hand_{side}", x=0, y=130, radius=18, fill=SKIN)
    arm_parts[side] += [h, crescent(h, -6, -6, SKIN_SH)]

# ---------------------------------------------------------------- head
for side, x in (("l", 164), ("r", 348)):
    ear = add("path", f"ear_{side}", d=ellipse(14, 20), tx=x, ty=278, fill=SKIN if side == "l" else SKIN_SH)
    add("path", f"ear_in_{side}", d=ellipse(6, 11), tx=x + (2 if side == "l" else -2), ty=279, fill=SKIN_DEEP)
face_d = "M-92 -20 C -92 -70 -52 -92 0 -92 C 52 -92 92 -70 92 -20 C 92 30 86 60 68 84 C 50 104 28 114 0 116 C -28 114 -50 104 -68 84 C -86 60 -92 30 -92 -20 Z"
face = add("path", "face", d=face_d, tx=256, ty=256, fill=SKIN)
neck_shadow = cast(neck, face, 0, 14, SKIN_DEEP)
face_side = crescent(face, -16, 0, SKIN_SH)
face_hi = crescent(face, 8, 10, SKIN_HI)

# hair geometry (built now for the cast shadow, moved to the front at the end)
hair_parts = [add("circle", "hair_core", x=256, y=170, radius=112)]
for i in range(22):
    a = 2 * math.pi * i / 22
    x, y = 256 + 118 * math.cos(a), 170 + 104 * math.sin(a)
    if y > 232:
        continue
    hair_parts.append(add("circle", f"hair_b{i}", x=round(x, 1), y=round(y, 1), radius=34 if i % 2 else 30))
for side, x in (("l", 166), ("r", 346)):
    hair_parts.append(add("circle", f"temple_{side}", x=x, y=222, radius=30))
hair_all = boolean("union", hair_parts)
window = add("path", "hairline", d="M-78 -36 C -42 -46 42 -46 78 -36 C 86 -28 90 -16 91 0 L 91 220 L -91 220 L -91 0 C -90 -16 -86 -28 -78 -36 Z", tx=256, ty=246)
hair = boolean("difference", [hair_all, window])
update(hair, fill=HAIR)
forehead = cast(face, hair, 4, 12, SKIN_SH)

# eyes: almond shaped, smaller iris, single highlight + dot, strong upper lid
sclera = "M-25 -6 C -17 -19 15 -21 25 -9 C 27 4 18 15 0 17 C -18 15 -27 4 -25 -6 Z"
eye_parts = {}
for side, x, f in (("l", 214, mx), ("r", 298, lambda d: d)):
    y = 284
    parts = [
        add("path", f"eye_{side}", d=f(sclera), tx=x, ty=y, fill="#FBF6EE"),
        add("path", f"iris_{side}", d=ellipse(14, 16), tx=x + 1, ty=y, gradient="linear 90 #3E1904 #9A5716 #E9C06A"),
        add("path", f"pupil_{side}", d=ellipse(6.5, 8), tx=x + 1, ty=y - 1, fill="#1E0B02"),
        add("path", f"hl1_{side}", d=ellipse(4.5, 5.5), tx=x - 5, ty=y - 6, fill="white"),
        add("circle", f"hl2_{side}", x=x + 7, y=y + 7, radius=2.2, fill="white"),
        add("path", f"lid_{side}", d=f("M-28 -6 C -18 -24 16 -26 28 -11 L30 -8 C 18 -18 -16 -16 -27 0 Z"), tx=x, ty=y, fill=FEATURE),
        add("path", f"lower_{side}", d=f("M-14 16 Q 0 20 12 15 L12 17 Q 0 23 -14 18 Z"), tx=x, ty=y, fill=SKIN_DEEP),
    ]
    eye_parts[side] = parts
    add("path", f"closed_{side}", d=f("M-26 0 Q 0 12 28 -2 L28 2 Q 0 17 -26 4 Z"), tx=x, ty=y, fill=FEATURE, opacity=0)
    add("path", f"brow_{side}", d=f("M-24 4 L-20 -4 L22 -6 L24 2 Z"), tx=x, ty=244, fill=HAIR_SH)
add("path", "nose", d="M3 -8 L-5 6 L3 8 L4 6 L-1 5 L5 -7 Z", tx=257, ty=310, fill=SKIN_DEEP)
add("path", "mouth", d="M-18 -2 Q 0 12 18 -4 L19 -1 Q 0 17 -18 1 Z", tx=256, ty=336, fill=FEATURE)
add("path", "mouth_corner", d="M18 -4 L22 -8 L23 -6 L19 -1 Z", tx=256, ty=336, fill=FEATURE)
add("path", "chin_shadow", d="M-26 0 Q 0 10 26 0 Q 0 6 -26 0 Z", tx=256, ty=356, fill=SKIN_SH, opacity=0.8)

# hair cel shading and curl texture, then bring the hair to the front
hair_sh = crescent(hair, -16, -14, HAIR_SH)
hair_hi = crescent(hair, 10, 12, HAIR_HI)
import random
rnd = random.Random(5)
curls = []
for i, (x, y) in enumerate([(178, 100), (220, 76), (262, 68), (304, 78), (342, 104), (150, 150), (370, 150),
                            (196, 130), (240, 112), (286, 116), (328, 136), (168, 196), (350, 196), (214, 170), (300, 172)]):
    curls.append(add("path", f"curl{i}", d="M-8 3 Q -6 -7 3 -6 Q 8 -4 6 3 L4 3 Q 5 -2 2 -3 Q -3 -4 -5 3 Z",
                     tx=x, ty=y, rotation=rnd.randint(-40, 40), scale_x=rnd.choice((0.8, 1.0, 1.2)),
                     scale_y=rnd.choice((0.8, 1.0, 1.2)), fill=HAIR_HI if x < 256 else HAIR_SH, opacity=0.9))
cli("arrange", "order", F, "--nodes", ",".join([hair, hair_sh, hair_hi, *curls]), "--to", "front")

# ---------------------------------------------------------------- firefly companion
fx, fy = 396, 94
ff = [
    add("circle", "ff_glow", x=fx, y=fy + 14, radius=40, gradient="radial rgba(236,255,140,0.85)@0 rgba(214,250,110,0.3)@0.45 rgba(200,240,90,0)@1", pivot="center"),
    add("path", "ff_wing_l", d="M0 0 C -6 -2 -24 -10 -26 -22 C -27 -32 -14 -30 -7 -22 C -3 -16 -1 -8 0 0 Z", tx=fx - 4, ty=fy - 2, fill="rgba(225,242,255,0.85)"),
    add("path", "ff_wing_r", d="M0 0 C 6 -2 24 -10 26 -22 C 27 -32 14 -30 7 -22 C 3 -16 1 -8 0 0 Z", tx=fx + 4, ty=fy - 2, fill="rgba(200,225,250,0.85)"),
    add("path", "ff_tail", d=ellipse(12, 13), tx=fx, ty=fy + 16, gradient="radial #FFFFF0@0 #F3FF9A@0.45 #BEEB3C@1", pivot="center"),
    add("circle", "ff_head", x=fx, y=fy, radius=13, gradient="linear 90 #5A63D8 #2E2F78"),
    add("circle", "ff_eye_l", x=fx - 4.5, y=fy - 1, radius=3, fill="white"),
    add("circle", "ff_eye_r", x=fx + 4.5, y=fy - 1, radius=3, fill="white"),
    add("circle", "ff_pupil_l", x=fx - 4, y=fy - 0.5, radius=1.6, fill="#1B1B3A"),
    add("circle", "ff_pupil_r", x=fx + 5, y=fy - 0.5, radius=1.6, fill="#1B1B3A"),
]

# ---------------------------------------------------------------- rig
cli("bone", "add", F, "--id", "torso", "--x", "0", "--y", "0", "--length", "1")
cli("bone", "add", F, "--id", "arm_l", "--parent", "torso", "--x", "144", "--y", "406", "--rotation", "11", "--length", "1")
cli("bone", "add", F, "--id", "arm_r", "--parent", "torso", "--x", "368", "--y", "406", "--rotation", "-11", "--length", "1")
cli("bone", "add", F, "--id", "lantern", "--parent", "arm_r", "--x", "-1", "--y", "130", "--rotation", "11", "--length", "1")
nodes = [n["id"] for n in cli("node", "list", F)["nodes"]]
arm = lambda s: arm_parts[s]
lantern = [n for n in nodes if n.startswith("lantern_")]
bound = set()
for s in "lr":
    for n in arm(s):
        cli("bone", "bind", F, "--node", n, "--bone", f"arm_{s}"); bound.add(n)
for n in lantern:
    cli("bone", "bind", F, "--node", n, "--bone", "lantern"); bound.add(n)
for n in nodes:
    if n not in bound and not n.startswith("ff_"):
        cli("bone", "bind", F, "--node", n, "--bone", "torso")
ff_group = cli("arrange", "group", F, "--nodes", ",".join(ff), "--name", "Gluehwuermchen")["id"]

# ---------------------------------------------------------------- animation
steps = []
shut = [f"--state {n}.opacity=0" for s in "lr" for n in eye_parts[s]] + ["--state closed_l.opacity=1", "--state closed_r.opacity=1"]
steps.append(f"on --machine blink --from open --to shut --after 3.8 {' '.join(shut)} --blend 0.06")
steps.append("on --machine blink --from shut --to open --after 0.13 --blend 0.06")
for s in "lr":
    for n in (f"iris_{s}", f"pupil_{s}", f"hl1_{s}", f"hl2_{s}"):
        steps.append(f"follow --target {n} --max 3 --strength 0.12 --smoothing 0.12")
steps += [
    "keyframe --clip breathe --target torso --property y --time 1.8 --by -3 --easing ease-in-out --autoplay --loop",
    "keyframe --clip breathe --target torso --property y --time 3.6 --value 0 --easing ease-in-out",
    "keyframe --clip sway --target arm_r --property rotation --time 1.2 --value -15 --easing ease-in-out --autoplay --loop",
    "keyframe --clip sway --target arm_r --property rotation --time 2.4 --value -11 --easing ease-in-out",
    "keyframe --clip sway --target lantern --property rotation --time 0.3 --value 11",
    "keyframe --clip sway --target lantern --property rotation --time 1.5 --value 19 --easing ease-in-out",
    "keyframe --clip sway --target lantern --property rotation --time 2.4 --value 11 --easing ease-in-out",
    "keyframe --clip sway --target arm_l --property rotation --time 1.2 --value 13 --easing ease-in-out",
    "keyframe --clip sway --target arm_l --property rotation --time 2.4 --value 11 --easing ease-in-out",
    f"keyframe --clip hover --target {ff_group} --property y --time 1.3 --by -10 --easing ease-in-out --autoplay --loop",
    f"keyframe --clip hover --target {ff_group} --property y --time 2.6 --value 0 --easing ease-in-out",
    "keyframe --clip flap --target ff_wing_l --property rotation --time 0.1 --value -18 --easing ease-in-out --autoplay --loop",
    "keyframe --clip flap --target ff_wing_l --property rotation --time 0.2 --value 0 --easing ease-in-out",
    "keyframe --clip flap --target ff_wing_r --property rotation --time 0.1 --value 18 --easing ease-in-out",
    "keyframe --clip flap --target ff_wing_r --property rotation --time 0.2 --value 0 --easing ease-in-out",
]
for n in ("ff_glow", "lantern_halo"):
    steps += [f"keyframe --clip glow --target {n} --property opacity --time 1.2 --value 0.5 --easing ease-in-out --autoplay --loop",
              f"keyframe --clip glow --target {n} --property opacity --time 2.4 --value 1 --easing ease-in-out"]
for i in range(3):
    t_on = round(0.35 + i * 0.5, 2)
    for n, lo in ((f"pin_glow{i}", 0.15), (f"pin{i}", 0.55)):
        extra = " --autoplay --loop" if (i == 0 and n.startswith("pin_glow")) else ""
        steps += [f"keyframe --clip loci --target {n} --property opacity --time 0.01 --value {lo}{extra}",
                  f"keyframe --clip loci --target {n} --property opacity --time {t_on} --value 1 --easing ease-out",
                  f"keyframe --clip loci --target {n} --property opacity --time 2.4 --value 1",
                  f"keyframe --clip loci --target {n} --property opacity --time 3 --value {lo} --easing ease-in"]
steps += [
    "clip add --id hop --duration 0.7",
    f"keyframe --clip hop --target {ff_group} --property rotation --time 0.7 --value 360 --easing ease-in-out",
    "on --event click --target face --play hop",
    'a11y --title "Simo" --description "Simo, das Maskottchen von Locora: ein Mann mit hellbraunem Hautton und Afro in voller Größe, mit einer Laterne voller Glühwürmchen-Licht und drei leuchtenden Loci an der Jacke."',
    'a11y --node face --label "Simo: klicken, dann dreht das Glühwürmchen eine Runde"',
]
tmp = F + ".steps"
open(tmp, "w").write("\n".join(steps) + "\n")
try:
    cli("batch", F, "--steps", tmp)
finally:
    os.remove(tmp)
print(f"built {F}")
