#!/usr/bin/env bash
# Regenerates the committed PNG icons in public/ from the SVG sources (needs Inkscape).
set -euo pipefail
cd "$(dirname "$0")/.."

export_png() { inkscape "$1" --export-type=png --export-filename="$2" -w "$3" -h "$3" >/dev/null 2>&1; }

export_png public/icon.svg public/icon-192.png 192
export_png public/icon.svg public/icon-512.png 512
export_png scripts/icons/apple-touch-icon.svg public/apple-touch-icon.png 180
export_png scripts/icons/icon-maskable.svg public/icon-maskable-512.png 512
