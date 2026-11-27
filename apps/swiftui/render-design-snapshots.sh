#!/bin/sh
# Renders the tracker window offscreen with fixture data into PNGs for design reviews.
# Pass --quick-track as the second argument to render only the Quick Track dialog.
# Set SNAPSHOT_FILTER to render only the scenarios whose name starts with it, such as sheet-.
# It doesn't launch the app, open a visible window or take keyboard focus.
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "Design snapshots require macOS and Xcode." >&2
    exit 1
fi

app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
out_dir=${1:-"$app_dir/design-snapshots"}
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM

swift_flags="-swift-version 6 -default-isolation MainActor -enable-upcoming-feature NonisolatedNonsendingByDefault -enable-upcoming-feature InferIsolatedConformances"
export MACOSX_DEPLOYMENT_TARGET=27.0

# shellcheck disable=SC2086
find "$app_dir/TrackerClient/Sources/TrackerClient" -name '*.swift' -print0 | xargs -0 xcrun swiftc $swift_flags \
    -parse-as-library -module-name TrackerClient -emit-module -emit-module-path "$build_dir/TrackerClient.swiftmodule" \
    -emit-library -static -o "$build_dir/libTrackerClient.a"

# The app sources without the entry point, the runtime and the Rust bridge, which the snapshot replaces.
# shellcheck disable=SC2086
find "$app_dir/Sources/TimeTrackerSwiftUI" -name '*.swift' \
    ! -name TimeTrackerApp.swift ! -name TrackerApplicationDelegate.swift \
    ! -name TrackerApplicationMenus.swift ! -name TrackerWorker.swift -print0 |
    xargs -0 xcrun swiftc $swift_flags -parse-as-library -D DEBUG -I "$build_dir" -L "$build_dir" -lTrackerClient \
        "$app_dir/Tests/DesignSnapshots/DesignSnapshots.swift" \
        -framework AppKit -framework SwiftUI -framework Carbon -o "$build_dir/design-snapshots"

"$build_dir/design-snapshots" "$out_dir" ${2:+"$2"}
