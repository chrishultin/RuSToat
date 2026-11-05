#!/usr/bin/env bash
# Build the app bundle (scripts/bundle.sh) and run it, with logs in this
# terminal. Running the bundle's executable directly, rather than with
# `open`, keeps stdout and stderr here.
#
# Usage: scripts/run-bundled.sh [debug|release]   (default: debug)
# Set RUST_LOG to change logging (default: info).
set -euo pipefail

app="$("$(dirname "$0")/bundle.sh" "${1:-debug}" | tail -n 1)"
export RUST_LOG="${RUST_LOG:-info}"
exec "$app/Contents/MacOS/stoat-gpui"
