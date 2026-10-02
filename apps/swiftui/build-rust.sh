#!/bin/bash
set -eo pipefail

: "${PROJECT_DIR:?Xcode must set PROJECT_DIR}"
: "${TARGET_TEMP_DIR:?Xcode must set TARGET_TEMP_DIR}"
: "${CONFIGURATION:?Xcode must set CONFIGURATION}"
if [ -z "${ARCHS:-}" ]; then
    printf 'Xcode did not provide an architecture in ARCHS.\n' >&2
    exit 1
fi

export PATH="$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:$PATH"

repository_dir=$(CDPATH= cd -- "$PROJECT_DIR/../.." && pwd)
rust_target_dir="$TARGET_TEMP_DIR/rust"
output_library="$TARGET_TEMP_DIR/libtracker_swift_bridge.a"
tracker_cargo_bin=${TRACKER_CARGO_BIN:-cargo}
tracker_lipo_bin=${TRACKER_LIPO_BIN:-lipo}
libraries=()
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

for architecture in $ARCHS; do
    case "$architecture" in
        arm64) rust_target=aarch64-apple-darwin ;;
        x86_64) rust_target=x86_64-apple-darwin ;;
        *) printf 'Unsupported macOS architecture: %s\n' "$architecture" >&2; exit 1 ;;
    esac

    "$tracker_cargo_bin" build --manifest-path "$repository_dir/Cargo.toml" --locked \
        -p tracker-swift-bridge --target "$rust_target" \
        --target-dir "$rust_target_dir" "${cargo_flags[@]}"
    library="$rust_target_dir/$rust_target/$profile/libtracker_swift_bridge.a"
    libraries+=("$library")
done

mkdir -p "$TARGET_TEMP_DIR"
if [ "${#libraries[@]}" -eq 1 ]; then
    cp "${libraries[0]}" "$output_library"
else
    # Xcode may ask for both Apple Silicon and Intel slices in a Release build.
    "$tracker_lipo_bin" -create "${libraries[@]}" -output "$output_library"
fi
