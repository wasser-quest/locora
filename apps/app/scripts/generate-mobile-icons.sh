#!/usr/bin/env bash
set -euo pipefail

app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$app_dir"
command -v magick >/dev/null 2>&1 || {
  printf 'ImageMagick (magick) fehlt für deckende iOS-Icons.\n' >&2
  exit 1
}

# Tauri writes Android icons directly into an existing gen/android project.
# Generate outside that project, then copy the complete, versioned icon set.
output="$app_dir/../../target/mobile-icon-output"
cargo tauri icon ../../assets/brand/mobile-icon.json -o "$output"
# App Store icons must not contain transparent pixels. Tauri's compositing can
# leave alpha at 254 on antialiased edges, so flatten the iOS outputs.
for icon in "$output"/ios/*.png; do
  magick "$icon" -background '#19183A' -alpha remove -alpha off "$icon"
done
cp -R "$output"/. src-tauri/icons/
printf 'Mobile icons: %s\n' "$app_dir/src-tauri/icons"
