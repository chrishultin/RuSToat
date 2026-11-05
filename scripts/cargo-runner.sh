#!/usr/bin/env bash
# Cargo runner (see .cargo/config.toml): signs the app binary before
# `cargo run` starts it, so the Keychain recognises it across rebuilds.
# Test and example binaries run unsigned, as before.
#
# Usage (by cargo): scripts/cargo-runner.sh <executable> [args...]
set -euo pipefail

exe="$1"
if [ "$(basename "$exe")" = "stoat-gpui" ]; then
  "$(dirname "$0")/sign.sh" "$exe" >/dev/null
fi
exec "$@"
