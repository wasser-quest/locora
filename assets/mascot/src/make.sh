#!/usr/bin/env bash
# Builds mnemo.sply from build.py with splinely-cli (stages 1-4).
set -euo pipefail
C=${SPLINELY_CLI:-splinely-cli}
OUT=${1:-mnemo.sply}
cd "$(dirname "$0")"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
rm -f "$OUT"
for st in 1 2 3; do python3 build.py "$tmp/s$st.txt" $st; "$C" batch "$OUT" --steps "$tmp/s$st.txt" >/dev/null; done
ff=$("$C" arrange group "$OUT" --nodes ff_glow,ff_wing_l,ff_wing_r,ff_tail,ff_head,ff_eye_l,ff_eye_r,ff_pupil_l,ff_pupil_r --name Gluehwuermchen | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
rest=$("$C" node list "$OUT" | python3 -c 'import json,sys;print(",".join(n["id"] for n in json.load(sys.stdin)["nodes"] if not n["id"].startswith("ff_")))')
fig=$("$C" arrange group "$OUT" --nodes "$rest" --name Figur | python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])')
python3 build.py "$tmp/s4.txt" 4
sed -i "s/--target figure /--target $fig /g; s/--target firefly /--target $ff /g" "$tmp/s4.txt"
"$C" batch "$OUT" --steps "$tmp/s4.txt" >/dev/null
echo "built $OUT"
