#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$script_dir/../.." && pwd)
rust_build_dir="$script_dir/.build/rust"
swift_build_dir="$script_dir/.build/swift"
bundle_dir="$script_dir/.build/Time Tracker.app"
rust_host=$(rustc -vV | sed -n 's/^host: //p')
rust_lib_dir="$rust_build_dir/$rust_host/release"

cd "$repo_dir"
cargo build --locked --release -p tracker-swift-bridge --target "$rust_host" \
    --target-dir "$rust_build_dir"

swift build --package-path "$script_dir" --scratch-path "$swift_build_dir" \
    --configuration release --product TimeTrackerSwiftUI \
    -Xlinker "-L$rust_lib_dir"

binary_dir=$(swift build --package-path "$script_dir" --scratch-path "$swift_build_dir" \
    --configuration release --show-bin-path)
mkdir -p "$bundle_dir/Contents/MacOS"
cp "$binary_dir/TimeTrackerSwiftUI" "$bundle_dir/Contents/MacOS/TimeTrackerSwiftUI"
cp "$script_dir/Info.plist" "$bundle_dir/Contents/Info.plist"
codesign --force --sign - --timestamp=none "$bundle_dir"

printf 'Built %s\n' "$bundle_dir"
