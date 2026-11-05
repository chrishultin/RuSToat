#!/usr/bin/env bash
# Code-sign a binary or app bundle as RuSToat (identifier com.chrishultin.rustoat).
#
# macOS ties Keychain access to an app's designated requirement. An ad-hoc
# signature's requirement is a hash of the binary, which changes on every
# build, so the Keychain asks again after each rebuild. A certificate
# signature with a fixed identifier keeps the requirement the same, so
# "Always Allow" sticks.
#
# The identity is $RUSTOAT_SIGN_IDENTITY if set, else the first
# "Apple Development:" identity in the keychain, else ad hoc (with a warning).
#
# Usage: scripts/sign.sh <path> [extra codesign options...]
# Prints the identity used.
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "usage: $0 <path> [codesign options...]" >&2
  exit 2
fi
path="$1"
shift

identity="${RUSTOAT_SIGN_IDENTITY:-}"
if [ -z "$identity" ]; then
  identity="$(security find-identity -v -p codesigning 2>/dev/null |
    sed -n 's/.*"\(Apple Development: .*\)"$/\1/p' | head -n 1)"
fi
if [ -z "$identity" ]; then
  echo "warning: no Apple Development signing identity found; signing ad hoc." >&2
  echo "warning: the Keychain will ask again after every rebuild." >&2
  identity="-"
fi

# Without options from the caller, skip the secure timestamp: it needs the
# network and isn't needed for day-to-day builds.
if [ $# -eq 0 ]; then
  set -- --timestamp=none
fi

codesign --force --sign "$identity" --identifier com.chrishultin.rustoat "$@" "$path"
echo "$identity"
