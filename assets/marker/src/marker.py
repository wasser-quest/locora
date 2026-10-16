#!/usr/bin/env python3
"""Builds the default locus marker (assets/marker/locus.sply) with splinely-cli.

Usage: SPLINELY_CLI=splinely-cli python3 assets/marker/src/marker.py [OUTPUT]

A marker is placed by the app at every locus position (x, y); room backdrops
contain no loci. Contract for custom markers (see assets/STIL.md):
- canvas 96 x 96, the locus is the centre (48, 48);
- state machine `status` with the states `neu`, `faellig` and `gewusst`
  (start: `neu`); the app switches them with the events of the same name;
- clicking the marker is handled by the app (no `click` interaction needed).
"""
import json, os, subprocess, sys

CLI = os.environ.get("SPLINELY_CLI", "splinely-cli")
F = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "locus.sply"))
CREAM, INK, LIME = "#F3EEE3", "#23245E", "#DDF76A"


def cli(*args):
    out = subprocess.run([CLI, *args], capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"splinely-cli {' '.join(args)}\n{out.stdout}{out.stderr}")
    return json.loads(out.stdout) if out.stdout.strip() else {}


if os.path.exists(F):
    os.remove(F)
cli("new", "--output", F, "--width", "96", "--height", "96", "--name", "Locus")
steps = [
    'node add --kind circle --id glow --x 48 --y 48 --radius 44 --pivot center --opacity 0.25 --gradient "radial rgba(230,255,120,0.85)@0 rgba(230,255,120,0)@1"',
    f'node add --kind circle --id ring --x 48 --y 48 --radius 20 --fill {INK} --opacity 0.55 --pivot center',
    f'node add --kind circle --id core --x 48 --y 48 --radius 15 --pivot center --gradient "radial #FFFFFF@0 {CREAM}@1"',
    # second core for the lime states; colours of gradient shapes cannot be animated, opacity can
    f'node add --kind circle --id core_lit --x 48 --y 48 --radius 15 --pivot center --opacity 0 --gradient "radial #FFFFF0@0 {LIME}@0.6 #A8CF2E@1"',
    f'node add --kind circle --id spark --x 43 --y 43 --radius 4 --fill white --opacity 0.9',
    # neu: cream core, faint halo; faellig: bright lime, pulsing halo; gewusst: calm lime, small halo
    "on --event faellig --machine status --from neu --to faellig --state core_lit.opacity=1 --state glow.opacity=0.95 --state ring.opacity=0.8 --blend 0.3",
    "on --event gewusst --machine status --from neu --to gewusst --state core_lit.opacity=0.75 --state glow.opacity=0.35 --state ring.opacity=0.3 --blend 0.3",
    "on --event neu --machine status --from faellig --to neu --blend 0.3",
    "on --event gewusst --machine status --from faellig --to gewusst --state core_lit.opacity=0.75 --state glow.opacity=0.35 --state ring.opacity=0.3 --blend 0.3",
    "on --event neu --machine status --from gewusst --to neu --blend 0.3",
    "on --event faellig --machine status --from gewusst --to faellig --state core_lit.opacity=1 --state glow.opacity=0.95 --state ring.opacity=0.8 --blend 0.3",
    # a gentle breathing glow in every state
    "keyframe --clip breathe --target glow --property scaleX --time 1.2 --value 1.15 --easing ease-in-out --autoplay --loop",
    "keyframe --clip breathe --target glow --property scaleX --time 2.4 --value 1 --easing ease-in-out",
    "keyframe --clip breathe --target glow --property scaleY --time 1.2 --value 1.15 --easing ease-in-out",
    "keyframe --clip breathe --target glow --property scaleY --time 2.4 --value 1 --easing ease-in-out",
    'a11y --title "Locus" --description "Ein leuchtender Punkt markiert einen Ort; neu, fällig oder gewusst."',
]
tmp = F + ".steps"
with open(tmp, "w") as f:
    f.write("\n".join(steps) + "\n")
try:
    cli("batch", F, "--steps", tmp)
finally:
    os.remove(tmp)
print("built", os.path.relpath(F))
