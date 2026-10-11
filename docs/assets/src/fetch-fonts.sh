#!/usr/bin/env bash
# Downloads the OFL fonts that build.mjs outlines into the SVG (not committed).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p fonts
base=https://raw.githubusercontent.com/google/fonts/main/ofl
for file in firasans/FiraSans-Regular.ttf firasans/FiraSans-Medium.ttf \
  firasans/FiraSans-SemiBold.ttf firasans/FiraSans-Bold.ttf firasans/FiraSans-Black.ttf \
  firamono/FiraMono-Regular.ttf firamono/FiraMono-Medium.ttf "caveat/Caveat%5Bwght%5D.ttf"; do
  name=$(basename "$file" | sed 's/%5B/[/; s/%5D/]/')
  [ -s "fonts/$name" ] || curl -fsSL "$base/$file" -o "fonts/$name"
done
