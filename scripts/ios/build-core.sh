#!/usr/bin/env bash
# Build the Rust core (ios/stoat-ffi) for iOS: a release static library per
# target, wrapped in ios/build/StoatCore.xcframework, plus its Swift bindings
# in ios/RuSToat/Generated/.
#
#   scripts/ios/build-core.sh            # device and simulator
#   scripts/ios/build-core.sh device     # aarch64-apple-ios only
#   scripts/ios/build-core.sh simulator  # aarch64-apple-ios-sim only
#
# Xcode runs this before each build (ios/project.yml) with PLATFORM_NAME set,
# building only the slice it needs. Cargo's own change tracking makes that
# quick when no Rust changed; the XCFramework is only rewritten when a
# library did.
set -euo pipefail

which="${1:-}"
if [ -z "$which" ]; then
    case "${PLATFORM_NAME:-}" in
        iphoneos) which=device ;;
        iphonesimulator) which=simulator ;;
        *) which=all ;;
    esac
fi

# Start again with a clean environment. Xcode exports its SDK, toolchain
# and deployment settings, which leak into the C parts of the build (aws-lc)
# for the wrong platform. And the C build scripts track variables like CC
# and SDKROOT, so any difference between Xcode's environment and a
# terminal's makes Cargo rebuild everything when switching between them.
if [ -z "${RUSTOAT_CORE_CLEAN_ENV:-}" ]; then
    exec env -i HOME="$HOME" USER="${USER:-}" TMPDIR="${TMPDIR:-/tmp}" \
        PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin" \
        RUSTOAT_CORE_CLEAN_ENV=1 \
        "$0" "$which"
fi

# Xcode's script phases don't see your shell's PATH.
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
case "$which" in
    device) targets=(aarch64-apple-ios) ;;
    simulator) targets=(aarch64-apple-ios-sim) ;;
    all) targets=(aarch64-apple-ios aarch64-apple-ios-sim) ;;
    *) echo "usage: $0 [device|simulator|all]" >&2; exit 2 ;;
esac

build="ios/build"
xcframework="$build/StoatCore.xcframework"
generated="ios/RuSToat/Generated"
lib="libstoat_ffi.a"

# The C parts (aws-lc) are built for the app's minimum iOS version.
export IPHONEOS_DEPLOYMENT_TARGET=26.0

for target in "${targets[@]}"; do
    cargo build -p stoat-ffi --lib --release --target "$target"
done

# Every slice that has been built, so a simulator-only run keeps an earlier
# device slice (and the other way round).
slices=()
for target in aarch64-apple-ios aarch64-apple-ios-sim; do
    if [ -f "target/$target/release/$lib" ]; then
        slices+=("target/$target/release/$lib")
    fi
done

stale=0
if [ ! -d "$xcframework" ] || [ ! -f "$generated/StoatCore.swift" ]; then
    stale=1
else
    for slice in "${slices[@]}"; do
        if [ "$slice" -nt "$xcframework/Info.plist" ]; then
            stale=1
        fi
    done
fi
if [ "$stale" = 0 ]; then
    echo "StoatCore.xcframework is up to date."
    exit 0
fi

# Bindings, read from the library's own metadata so they always match it.
bindings="$build/bindings"
rm -rf "$bindings"
cargo run -q -p stoat-ffi --features bindgen --bin uniffi-bindgen -- \
    generate --library "${slices[0]}" --language swift --out-dir "$bindings"

headers="$build/headers"
rm -rf "$headers"
mkdir -p "$headers"
cp "$bindings/StoatCoreFFI.h" "$headers/"
cp "$bindings/StoatCoreFFI.modulemap" "$headers/module.modulemap"

args=()
for slice in "${slices[@]}"; do
    args+=(-library "$slice" -headers "$headers")
done
rm -rf "$xcframework"
xcodebuild -create-xcframework "${args[@]}" -output "$xcframework" >/dev/null

mkdir -p "$generated"
cp "$bindings/StoatCore.swift" "$generated/StoatCore.swift"
echo "Built $xcframework (${#slices[@]} slice(s)) and $generated/StoatCore.swift."
