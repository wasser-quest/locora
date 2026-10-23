#!/usr/bin/env bash
set -euo pipefail

app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$app_dir"
command -v magick >/dev/null 2>&1 || {
  printf 'ImageMagick (magick) fehlt für deckende iOS-Icons und die alten Android-Launcher-Icons.\n' >&2
  exit 1
}

# Tauri writes Android icons directly into an existing gen/android project.
# Generate outside that project, then copy the complete, versioned icon set.
output="$app_dir/../../target/mobile-icon-output"
logo=../../assets/brand/icon.png
cargo tauri icon ../../assets/brand/mobile-icon.json -o "$output"
# App Store icons must not contain transparent pixels. Tauri's compositing can
# leave alpha at 254 on antialiased edges, so flatten the iOS outputs. -strip
# drops ImageMagick's time stamps, so the same source gives the same files.
for icon in "$output"/ios/*.png; do
  magick "$icon" -background '#19183A' -alpha remove -alpha off -strip "$icon"
done
# Legacy launcher icons (Android 7.x, before adaptive icons): with an adaptive
# foreground, Tauri composes them from both layers over the whole 108 dp, which
# leaves the logo at about a third of the icon. Rebuild them from the logo in
# Tauri's frames: the square icon 40 of 48 dp (inset 1/12), the round icon a
# circle of 44 of 48 dp (inset 1/24). The logo's rounded square contains its
# inscribed circle, so the round icon is the logo cut to that circle.
for density in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
  dir="$output/android/mipmap-${density%%:*}"
  size="${density##*:}"
  square=$((size * 5 / 6))
  round=$((size * 11 / 12))
  centre="$((round / 2 - 1)).5" # pixel centres are integers: (round - 1) / 2
  magick "$logo" -resize "${square}x${square}" \
    -background none -gravity center -extent "${size}x${size}" \
    -strip "PNG32:$dir/ic_launcher.png"
  magick "$logo" -resize "${round}x${round}" \
    \( -size "${round}x${round}" xc:none -fill white -draw "circle $centre,$centre $centre,-0.5" \) \
    -compose DstIn -composite -compose Over \
    -background none -gravity center -extent "${size}x${size}" \
    -strip "PNG32:$dir/ic_launcher_round.png"
done
cp -R "$output"/. src-tauri/icons/
printf 'Mobile icons: %s\n' "$app_dir/src-tauri/icons"
