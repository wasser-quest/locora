#!/usr/bin/env bash
# Exports of the Locora logo, assets/brand/icon.sply (the website's favicon.svg at 512 x 512),
# for places without the Splinely runtime:
#   brand/icon.svg              interactive SVG (the glow pulses)
#   brand/icon.png              512 px at time 0: the source of the desktop, Android and iOS icons
#   brand/icon-foreground.png   Android adaptive-icon foreground: the art alone, inside the safe zone
#   apps/web/shell/icons/*.png  the web app's icons (any, maskable, apple-touch)
# The platform icons are made from these by apps/app/scripts/generate-mobile-icons.sh.
# Needs splinely-cli and ImageMagick (magick).
set -euo pipefail

brand="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
web="$brand/../../apps/web/shell/icons"
for tool in splinely-cli magick; do
  command -v "$tool" >/dev/null 2>&1 || { printf '%s is missing.\n' "$tool" >&2; exit 1; }
done
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cli() { splinely-cli "$@" >/dev/null; }
# magick ARGS... OUTPUT without metadata (time stamps), so the same source gives the same files.
png() { magick "${@:1:$#-1}" -strip "${@: -1}"; }

cli check "$brand/icon.sply"
cli export "$brand/icon.sply" --output "$brand/icon.svg"
cli render "$brand/icon.sply" --time 0 --output "$brand/icon.png"

# The art alone (no background), and the logo without rounded corners (iOS and the
# launchers cut their own shape).
cli node delete "$brand/icon.sply" --nodes bg --output "$tmp/art.sply"
cli node update "$brand/icon.sply" bg --rx 0 --output "$tmp/square.sply"
cli node delete "$tmp/square.sply" --nodes arch,floor,lumi_glow,lumi,locus --output "$tmp/bg.sply"

# Android: the foreground layer is 108 dp, only a circle of 66 dp is safe from the mask. The
# art's farthest point (a corner of the floor) is 221.3 px from the centre of the 512 canvas;
# at 352/512 that is 152.1 px, inside 512 * 33/108 = 156.4 px. The background layer is
# mobile-icon-background.svg (the logo's gradient).
cli render "$tmp/art.sply" --time 0 --width 352 --output "$tmp/art-352.png"
png "$tmp/art-352.png" -background none -gravity center -extent 512x512 "$brand/icon-foreground.png"

# Web app: the manifest's icons, and the apple-touch icon, which iOS rounds itself.
cli render "$brand/icon.sply" --time 0 --width 192 --output "$web/icon-192.png"
cli render "$brand/icon.sply" --time 0 --width 512 --output "$web/icon-512.png"
cli render "$tmp/square.sply" --time 0 --width 180 --output "$web/apple-touch-icon.png"
# Maskable: full-bleed background, the art inside the safe circle (radius 40 %, 204.8 px):
# 221.3 px at 460/512 is 198.8 px.
cli render "$tmp/bg.sply" --time 0 --width 512 --output "$tmp/bg-512.png"
cli render "$tmp/art.sply" --time 0 --width 460 --output "$tmp/art-460.png"
png "$tmp/bg-512.png" "$tmp/art-460.png" -gravity center -composite "$web/icon-maskable-512.png"
cp "$brand/favicon.svg" "$web/favicon.svg"
printf 'Logo exports: %s, %s\n' "$brand" "$(cd "$web" && pwd)"
