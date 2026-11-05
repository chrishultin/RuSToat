#!/usr/bin/env bash
# Build RuSToat and wrap it in a minimal macOS app bundle.
#
# macOS only lets an app post notification banners when it runs from a
# bundle with an identifier; a plain `cargo run` binary gets in-app toasts
# instead. The bundle is signed by scripts/sign.sh (your Apple Development
# identity, or ad hoc without one), which is enough to run locally.
#
# Usage: scripts/bundle.sh [debug|release]   (default: debug)
# Prints the bundle's path as the last line of output.
set -euo pipefail

cd "$(dirname "$0")/.."

profile="${1:-debug}"
case "$profile" in
  debug) cargo build ;;
  release) cargo build --release ;;
  *)
    echo "usage: $0 [debug|release]" >&2
    exit 2
    ;;
esac

target="${CARGO_TARGET_DIR:-target}/$profile"
app="$target/RuSToat.app"
version="$(cargo pkgid | sed 's/.*[#@]//')"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target/stoat-gpui" "$app/Contents/MacOS/stoat-gpui"
# The icon (regenerate it from assets/icon/rustoat.svg with scripts/make-icon.sh).
cp assets/icon/RuSToat.icns "$app/Contents/Resources/RuSToat.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.chrishultin.rustoat</string>
  <key>CFBundleName</key>
  <string>RuSToat</string>
  <key>CFBundleDisplayName</key>
  <string>RuSToat</string>
  <key>CFBundleIconFile</key>
  <string>RuSToat</string>
  <key>CFBundleExecutable</key>
  <string>stoat-gpui</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

identity="$(scripts/sign.sh "$app")"
# Finder and the Dock cache icons by bundle; a fresh modification date
# nudges them to look again.
touch "$app"
echo "signed as: $identity" >&2
echo "$app"
