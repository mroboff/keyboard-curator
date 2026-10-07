#!/usr/bin/env bash
# Renders the app icon from its SVG source into the .icns the bundle uses.
#
# Usage: ci/make-icon.sh
#
# Reads crates/kc-app/assets/icon/icon.svg and writes icon.icns beside it.
# The SVG is rasterized once at 1024x1024 by headless Chrome (the only
# renderer on a stock Mac that handles SVG faithfully), the smaller sizes
# are made with sips, and iconutil packs them. The result is checked in,
# so this only needs running when the SVG changes.
set -euo pipefail

cd "$(dirname "$0")/.."
icon_dir=crates/kc-app/assets/icon
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A page that shows the SVG edge to edge on a transparent background, so
# the screenshot is the icon itself with its margins clear.
cat > "$work/icon.html" <<HTML
<!doctype html>
<html><head><meta charset="utf-8"><style>
html, body { margin: 0; padding: 0; background: transparent; }
img { display: block; width: 1024px; height: 1024px; }
</style></head>
<body><img src="file://$PWD/$icon_dir/icon.svg"></body></html>
HTML

"$chrome" --headless=new --disable-gpu --hide-scrollbars \
    --default-background-color=00000000 \
    --window-size=1024,1024 \
    --screenshot="$work/icon-1024.png" \
    "file://$work/icon.html" 2>/dev/null

mkdir "$work/icon.iconset"
for size in 16 32 128 256 512; do
    double=$((size * 2))
    sips -z "$size" "$size" "$work/icon-1024.png" --out "$work/icon.iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$double" "$double" "$work/icon-1024.png" --out "$work/icon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done

iconutil -c icns "$work/icon.iconset" -o "$icon_dir/icon.icns"
echo "Wrote $icon_dir/icon.icns"
