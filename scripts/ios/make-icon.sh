#!/usr/bin/env bash
# Regenerate the iPhone app's icon (ios/RuSToat/Assets.xcassets) from
# assets/icon/rustoat.svg, as one 1024 px PNG that iOS scales and rounds.
# The PNG is committed; run this after editing the SVG.
set -euo pipefail
cd "$(dirname "$0")/../.."
out=ios/RuSToat/Assets.xcassets/AppIcon.appiconset/AppIcon.png
cargo run --quiet --example render_icon -- --ios assets/icon/rustoat.svg "$out"
echo "$out"
