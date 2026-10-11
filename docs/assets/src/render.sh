#!/usr/bin/env bash
# Rasterizes ../archon_graphic.svg to ../archon_graphic.png at 1.5x with headless Chrome.
set -euo pipefail
cd "$(dirname "$0")"
chrome=${CHROME:-$(ls -d "$HOME"/.cache/puppeteer/chrome-headless-shell/*/chrome-headless-shell-linux64/chrome-headless-shell 2>/dev/null | tail -n1)}
[ -x "$chrome" ] || { echo "set CHROME to a Chrome or chrome-headless-shell binary" >&2; exit 1; }
read -r width height < <(sed -nE 's/.*<svg[^>]* width="([0-9]+)" height="([0-9]+)".*/\1 \2/p' ../archon_graphic.svg | head -n1)
[ -n "${width:-}" ] || { echo "could not read width/height from archon_graphic.svg" >&2; exit 1; }
profile=$(pwd)/.chrome-profile
"$chrome" --headless --disable-gpu --hide-scrollbars --user-data-dir="$profile" \
  --force-device-scale-factor=1.5 --window-size="$width,$height" \
  --screenshot="$(pwd)/../archon_graphic.png" "file://$(pwd)/render.html" 2>/dev/null
rm -rf "$profile"
