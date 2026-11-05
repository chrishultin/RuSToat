#!/usr/bin/env bash
# Build a release RuSToat.app and package it as dist/RuSToat-<version>.dmg.
#
# Signing: "Developer ID Application" if the keychain has one, else
# "Apple Development" (the DMG then only runs on Macs registered to your
# team, such as this one). $RUSTOAT_SIGN_IDENTITY overrides the choice.
# Always with the hardened runtime and a secure timestamp.
#
# Notarization runs only when a Developer ID identity is used and
# $RUSTOAT_NOTARY_PROFILE names a notarytool keychain profile, created once
# with:
#   xcrun notarytool store-credentials <profile> --apple-id <you> --team-id <team>
#
# Usage: scripts/release.sh [--dry-run]
#   --dry-run  Build nothing; print the identity, the DMG path and the
#              notarization commands that would run.
# Prints the DMG's path as the last line of output.
set -euo pipefail

cd "$(dirname "$0")/.."

dry_run=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    *)
      echo "usage: $0 [--dry-run]" >&2
      exit 2
      ;;
  esac
done

# Run a command, or only print it in a dry run.
run() {
  if [ "$dry_run" = 1 ]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

identities="$(security find-identity -v -p codesigning 2>/dev/null || true)"
first_identity() {
  sed -n "s/.*\"\($1: .*\)\"\$/\1/p" <<<"$identities" | head -n 1
}

identity="${RUSTOAT_SIGN_IDENTITY:-}"
if [ -z "$identity" ]; then
  identity="$(first_identity "Developer ID Application")"
fi
if [ -z "$identity" ]; then
  identity="$(first_identity "Apple Development")"
fi
if [ -z "$identity" ] || [ "$identity" = "-" ]; then
  echo "error: no signing identity. A release needs a Developer ID Application" >&2
  echo "or Apple Development certificate in your keychain." >&2
  exit 1
fi

developer_id=0
case "$identity" in
  "Developer ID Application:"*) developer_id=1 ;;
  *)
    echo "warning: signing with '$identity', not Developer ID." >&2
    echo "warning: the DMG will only run on Macs registered to your team." >&2
    ;;
esac

version="$(cargo pkgid | sed 's/.*[#@]//')"
dmg="dist/RuSToat-$version.dmg"
echo "identity: $identity" >&2
echo "dmg: $dmg" >&2

# The bundle, signed for distribution.
if [ "$dry_run" = 1 ]; then
  run env RUSTOAT_SIGN_IDENTITY="$identity" scripts/bundle.sh release --release-signing
  app="target/release/RuSToat.app"
else
  app="$(RUSTOAT_SIGN_IDENTITY="$identity" scripts/bundle.sh release --release-signing | tail -n 1)"
fi
run codesign --verify --deep --strict "$app"

# The disk image: the app beside a link to /Applications, so it can be
# dragged across. Built from a fresh staging folder.
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
run ditto "$app" "$staging/RuSToat.app"
run ln -s /Applications "$staging/Applications"
run mkdir -p dist
run rm -f "$dmg"
# hdiutil can fail with "Resource busy" while Spotlight or a previous
# image still holds a file; try a few times.
create_dmg() {
  local attempt
  for attempt in 1 2 3; do
    if hdiutil create -volname RuSToat -srcfolder "$staging" -format UDZO -ov "$dmg"; then
      return 0
    fi
    echo "hdiutil create failed (attempt $attempt); retrying" >&2
    sleep 2
  done
  return 1
}
if [ "$dry_run" = 1 ]; then
  run hdiutil create -volname RuSToat -srcfolder "$staging" -format UDZO -ov "$dmg"
else
  create_dmg
fi
run codesign --force --sign "$identity" --timestamp "$dmg"

# Notarize and staple, when we can.
profile="${RUSTOAT_NOTARY_PROFILE:-}"
if [ "$developer_id" = 1 ] && [ -n "$profile" ]; then
  run xcrun notarytool submit "$dmg" --keychain-profile "$profile" --wait
  run xcrun stapler staple "$dmg"
elif [ -n "$profile" ]; then
  echo "note: not notarizing: RUSTOAT_NOTARY_PROFILE is set, but notarization" >&2
  echo "needs a Developer ID Application certificate (only the team's Account" >&2
  echo "Holder can create one at developer.apple.com → Certificates)." >&2
else
  echo "note: not notarizing. To notarize, sign with a Developer ID Application" >&2
  echo "certificate and set RUSTOAT_NOTARY_PROFILE to a profile stored with" >&2
  echo "'xcrun notarytool store-credentials'." >&2
fi

run hdiutil verify "$dmg"
echo "$dmg"
