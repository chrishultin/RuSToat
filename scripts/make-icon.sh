#!/usr/bin/env bash
# Regenerate assets/icon/RuSToat.icns from assets/icon/rustoat.svg.
#
# Renders every iconset size with examples/render_icon.rs (resvg), then packs
# them with macOS's iconutil. The .icns is committed, so bundling doesn't
# need this; run it after editing the SVG.
#
# Usage: scripts/make-icon.sh
set -euo pipefail

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
iconset="$work/RuSToat.iconset"

cargo run --quiet --example render_icon -- assets/icon/rustoat.svg "$iconset"
iconutil -c icns -o assets/icon/RuSToat.icns "$iconset"
echo assets/icon/RuSToat.icns
