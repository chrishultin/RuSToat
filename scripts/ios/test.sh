#!/usr/bin/env bash
# The iPhone app's checks: the core without `desktop`, the FFI crate's tests,
# and the Swift tests on a simulator.
#
#   scripts/ios/test.sh                 # first available iPhone simulator
#   IOS_SIMULATOR="iPhone 17" scripts/ios/test.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

cargo clippy --lib --no-default-features -- -D warnings
cargo check --lib --no-default-features --target aarch64-apple-ios
cargo test -p stoat-ffi
cargo clippy -p stoat-ffi --all-targets -- -D warnings

scripts/ios/build-core.sh
(cd ios && xcodegen generate --quiet)

# By UDID: names can repeat (one per installed iOS version).
list="$(xcrun simctl list devices available)"
if [ -n "${IOS_SIMULATOR:-}" ]; then
    udid="$(grep -F "    $IOS_SIMULATOR (" <<<"$list" | head -1 | sed -nE 's/.*\(([0-9A-F-]{36})\).*/\1/p')"
else
    udid="$(grep -E '^ +iPhone' <<<"$list" | head -1 | sed -nE 's/.*\(([0-9A-F-]{36})\).*/\1/p')"
fi
if [ -z "$udid" ]; then
    echo "No ${IOS_SIMULATOR:-iPhone} simulator found. Add one in Xcode → Settings → Components." >&2
    exit 1
fi
echo "Testing on $(grep -F "$udid" <<<"$list" | sed -E 's/^ +//')"

# Boot it first and wait: launching the test host while the simulator is
# still starting fails with "Application failed preflight checks".
xcrun simctl bootstatus "$udid" -b >/dev/null
xcodebuild -project ios/RuSToat.xcodeproj -scheme RuSToat \
    -destination "id=$udid" -quiet test
