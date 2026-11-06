#!/bin/bash
set -eo pipefail

: "${PROJECT_DIR:?Xcode must set PROJECT_DIR}"
: "${TARGET_TEMP_DIR:?Xcode must set TARGET_TEMP_DIR}"
: "${CONFIGURATION:?Xcode must set CONFIGURATION}"
if [ "${ARCHS:-}" != arm64 ]; then
    printf 'Time Tracker builds for arm64 only, but Xcode set ARCHS to: %s\n' "${ARCHS:-nothing}" >&2
    exit 1
fi

export PATH="$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:$PATH"

repository_dir=$(CDPATH= cd -- "$PROJECT_DIR/../.." && pwd)
rust_target=aarch64-apple-darwin
rust_target_dir="$TARGET_TEMP_DIR/rust"
output_library="$TARGET_TEMP_DIR/libtracker_swift_bridge.a"
tracker_cargo_bin=${TRACKER_CARGO_BIN:-cargo}
cargo_flags=()

case "$CONFIGURATION" in
    Release)
        profile=release
        cargo_flags=(--release)
        ;;
    *)
        profile=debug
        ;;
esac

"$tracker_cargo_bin" build --manifest-path "$repository_dir/Cargo.toml" --locked \
    -p tracker-swift-bridge --target "$rust_target" \
    --target-dir "$rust_target_dir" "${cargo_flags[@]}"

mkdir -p "$TARGET_TEMP_DIR"
cp "$rust_target_dir/$rust_target/$profile/libtracker_swift_bridge.a" "$output_library"
