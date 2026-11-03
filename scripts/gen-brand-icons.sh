#!/usr/bin/env bash
# Regenerate the raster brand icons in web/static/ from assets/brand/*.svg.
# Requires ImageMagick 7 (`magick`). The SVGs are the source of truth; rerun
# this after editing them and commit the resulting PNG/ICO files.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/assets/brand"
out="$root/web/static"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

render() { # <svg> <size> <png>
  magick -background none -density 1536 "$1" -resize "$2x$2" -strip -depth 8 "$3"
}

# Full-bleed square variant for platforms that apply their own mask
# (apple-touch-icon, maskable manifest icon): drop the tile's rounded corners
# and, for maskable, shrink the mark into the 80% safe zone.
sed 's/ rx="7"//' "$src/ironforge-icon.svg" > "$tmp/square.svg"
sed -e 's/ rx="7"//' \
    -e 's|<path fill="#eceff1"|<g transform="translate(16 16) scale(0.78) translate(-16 -16)"><path fill="#eceff1"|' \
    -e 's|</svg>|</g></svg>|' \
    "$src/ironforge-icon.svg" > "$tmp/maskable.svg"

cp "$src/ironforge-icon.svg" "$out/favicon.svg"
render "$src/ironforge-icon.svg" 192 "$out/icon-192.png"
render "$src/ironforge-icon.svg" 512 "$out/icon-512.png"
render "$tmp/square.svg" 180 "$out/apple-touch-icon.png"
render "$tmp/maskable.svg" 512 "$out/icon-maskable-512.png"

for s in 16 32 48; do render "$src/ironforge-icon.svg" "$s" "$tmp/ico-$s.png"; done
magick "$tmp/ico-16.png" "$tmp/ico-32.png" "$tmp/ico-48.png" "$out/favicon.ico"
