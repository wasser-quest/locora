#!/usr/bin/env python3
"""Builds Simo's firefly as a standalone figure with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/mascot/src/gluehwuermchen.py assets/mascot/gluehwuermchen.sply
Same design as the firefly next to Simo (assets/mascot/src/simo.py), scaled
up: no outlines, indigo head, big white eyes, tinted wings and a glowing
tail. Below it a route of three numbered loci lights up 1 -> 2 -> 3.
"""
import json, os, subprocess, sys

CLI = os.environ.get("SPLINELY_CLI", "splinely-cli")
F = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "gluehwuermchen.sply")
S = 4.4             # scale relative to the firefly next to Simo
FX, FY = 256, 196   # head centre
LIME = "#DDF76A"

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

def ellipse(rx, ry):
    k = 0.5523
    return (f"M{-rx:g} 0 C {-rx:g} {-ry*k:.2f} {-rx*k:.2f} {-ry:g} 0 {-ry:g} C {rx*k:.2f} {-ry:g} {rx:g} {-ry*k:.2f} {rx:g} 0 "
            f"C {rx:g} {ry*k:.2f} {rx*k:.2f} {ry:g} 0 {ry:g} C {-rx*k:.2f} {ry:g} {-rx:g} {ry*k:.2f} {-rx:g} 0 Z")

def scaled(d):
    import re
    return " ".join(t if t.isalpha() else f"{float(t)*S:.2f}".rstrip("0").rstrip(".")
                    for t in re.findall(r"[A-Za-z]|-?\d*\.?\d+", d))

if os.path.exists(F):
    os.remove(F)
cli("new", "--output", F, "--width", "512", "--height", "512", "--name", "Glühwürmchen")

# ---------------------------------------------------------------- firefly (Simo's design, scaled)
ff = [
    add("circle", "glow", x=FX, y=FY + 14 * S, radius=40 * S, pivot="center",
        gradient="radial rgba(236,255,140,0.85)@0 rgba(214,250,110,0.3)@0.45 rgba(200,240,90,0)@1"),
    add("path", "wing_l", d=scaled("M0 0 C -6 -2 -24 -10 -26 -22 C -27 -32 -14 -30 -7 -22 C -3 -16 -1 -8 0 0 Z"),
        tx=FX - 4 * S, ty=FY - 2 * S, fill="rgba(225,242,255,0.85)"),
    add("path", "wing_r", d=scaled("M0 0 C 6 -2 24 -10 26 -22 C 27 -32 14 -30 7 -22 C 3 -16 1 -8 0 0 Z"),
        tx=FX + 4 * S, ty=FY - 2 * S, fill="rgba(200,225,250,0.85)"),
    add("path", "tail", d=ellipse(12 * S, 13 * S), tx=FX, ty=FY + 16 * S, pivot="center",
        gradient="radial #FFFFF0@0 #F3FF9A@0.45 #BEEB3C@1"),
    add("circle", "head", x=FX, y=FY, radius=13 * S, gradient="linear 90 #5A63D8 #2E2F78"),
    add("circle", "eye_l", x=FX - 4.5 * S, y=FY - 1 * S, radius=3 * S, fill="white", pivot="center"),
    add("circle", "eye_r", x=FX + 4.5 * S, y=FY - 1 * S, radius=3 * S, fill="white", pivot="center"),
    add("circle", "pupil_l", x=FX - 4 * S, y=FY - 0.5 * S, radius=1.6 * S, fill="#1B1B3A"),
    add("circle", "pupil_r", x=FX + 5 * S, y=FY - 0.5 * S, radius=1.6 * S, fill="#1B1B3A"),
]
# closed eyes for blinking: a flat dark lid shape per eye
for side, x in (("l", FX - 4.5 * S), ("r", FX + 4.5 * S)):
    ff.append(add("path", f"closed_{side}", d=scaled("M-3 0 Q 0 2 3 0 L3 0.6 Q 0 2.8 -3 0.6 Z"),
                  tx=x, ty=FY - 0.5 * S, fill="#1B1B3A", opacity=0))
group = cli("arrange", "group", F, "--nodes", ",".join(ff), "--name", "Glühwürmchen")["id"]

# ---------------------------------------------------------------- loci route below: 1 -> 2 -> 3
route = [(84, 440), (156, 468), (228, 448)]
for i, (x, y) in enumerate(route[:-1]):
    nx, ny = route[i + 1]
    for k in (1, 2):
        t = k / 3
        add("circle", f"step{i}{k}", x=round(x + (nx - x) * t, 1), y=round(y + (ny - y) * t, 1), radius=3.5,
            fill=LIME, opacity=0.6)
for i, (x, y) in enumerate(route, start=1):
    add("circle", f"locus{i}_halo", x=x, y=y, radius=32, pivot="center", opacity=0.25,
        gradient="radial rgba(230,255,120,0.7)@0 rgba(230,255,120,0)@1")
    add("circle", f"locus{i}", x=x, y=y, radius=18, pivot="center", opacity=0.45,
        gradient=f"radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1")
    add("text", f"num{i}", x=x - 6, y=y + 7, text=str(i), font_size=20, fill="#2E2F78")
for i, (x, y, r) in enumerate([(92, 150, 14), (424, 118, 11), (436, 352, 16)]):
    add("star", f"spark{i}", x=x, y=y, radius=r, tips=4, inner=0.35, fill="#FFE27A", pivot="center", opacity=0.2)

# ---------------------------------------------------------------- animation
steps = [
    f"keyframe --clip hover --target {group} --property y --time 1.4 --by -12 --easing ease-in-out --autoplay --loop",
    f"keyframe --clip hover --target {group} --property y --time 2.8 --value 0 --easing ease-in-out",
    "keyframe --clip flap --target wing_l --property rotation --time 0.11 --value -16 --easing ease-in-out --autoplay --loop",
    "keyframe --clip flap --target wing_l --property rotation --time 0.22 --value 0 --easing ease-in-out",
    "keyframe --clip flap --target wing_r --property rotation --time 0.11 --value 16 --easing ease-in-out",
    "keyframe --clip flap --target wing_r --property rotation --time 0.22 --value 0 --easing ease-in-out",
    "keyframe --clip shine --target glow --property opacity --time 1.2 --value 0.5 --easing ease-in-out --autoplay --loop",
    "keyframe --clip shine --target glow --property opacity --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip shine --target tail --property scaleX --time 1.2 --value 1.04 --easing ease-in-out",
    "keyframe --clip shine --target tail --property scaleX --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip shine --target tail --property scaleY --time 1.2 --value 1.04 --easing ease-in-out",
    "keyframe --clip shine --target tail --property scaleY --time 2.4 --value 1 --easing ease-in-out",
    "on --machine blink --from open --to shut --after 3.2 --state eye_l.opacity=0 --state eye_r.opacity=0"
    " --state pupil_l.opacity=0 --state pupil_r.opacity=0 --state closed_l.opacity=1 --state closed_r.opacity=1 --blend 0.06",
    "on --machine blink --from shut --to open --after 0.14 --blend 0.06",
    "follow --target pupil_l --max 6 --strength 0.15 --smoothing 0.12",
    "follow --target pupil_r --max 6 --strength 0.15 --smoothing 0.12",
    "clip add --id hop --duration 0.6",
    f"keyframe --clip hop --target {group} --property y --time 0.25 --by -40 --easing ease-out",
    f"keyframe --clip hop --target {group} --property y --time 0.6 --value 0 --easing ease-in",
    "on --event click --target head --play hop",
]
for i in range(1, 4):
    on = round((i - 1) * 0.6 + 0.35, 2)
    for n, lo in ((f"locus{i}", 0.45), (f"locus{i}_halo", 0.25)):
        extra = " --autoplay --loop" if (i == 1 and n == "locus1") else ""
        steps += [f"keyframe --clip route --target {n} --property opacity --time {on} --value 1 --easing ease-out{extra}",
                  f"keyframe --clip route --target {n} --property opacity --time 2.6 --value 1",
                  f"keyframe --clip route --target {n} --property opacity --time 3.2 --value {lo} --easing ease-in"]
    steps += [f"keyframe --clip route --target locus{i} --property scaleX --time {on} --value 1.2 --easing ease-out",
              f"keyframe --clip route --target locus{i} --property scaleX --time {round(on + 0.3, 2)} --value 1 --easing ease-in",
              f"keyframe --clip route --target locus{i} --property scaleY --time {on} --value 1.2 --easing ease-out",
              f"keyframe --clip route --target locus{i} --property scaleY --time {round(on + 0.3, 2)} --value 1 --easing ease-in"]
for i, t in enumerate((0.8, 1.6, 2.4)):
    steps += [f"keyframe --clip twinkle --target spark{i} --property opacity --time {t} --value 1 --easing ease-out"
              + (" --autoplay --loop" if i == 0 else ""),
              f"keyframe --clip twinkle --target spark{i} --property opacity --time {round(t + 0.8, 2)} --value 0.2 --easing ease-in"]
steps += [
    'a11y --title "Glühwürmchen" --description "Simos Glühwürmchen: es schwebt, leuchtet und lässt die Loci einer Gedächtnisroute nacheinander aufleuchten, 1, 2, 3."',
    'a11y --node head --label "Glühwürmchen: klicken zum Hüpfen"',
    "a11y --node locus1 --label \"Locus 1\"", "a11y --node locus2 --label \"Locus 2\"", "a11y --node locus3 --label \"Locus 3\"",
]
tmp = F + ".steps"
open(tmp, "w").write("\n".join(steps) + "\n")
try:
    cli("batch", F, "--steps", tmp)
finally:
    os.remove(tmp)
print(f"built {F}")
