#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "Native layout checks require macOS and Xcode." >&2
    exit 1
fi

app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
check_dir=$(mktemp -d)
trap 'rm -rf "$check_dir"' EXIT HUP INT TERM

MACOSX_DEPLOYMENT_TARGET=13.0 xcrun swiftc -swift-version 5 -parse-as-library \
    "$app_dir/Sources/TimeTrackerSwiftUI/App/TrackerPaneViewController.swift" \
    "$app_dir/Tests/NativeLayout/PaneLayoutChecks.swift" \
    -framework AppKit -framework SwiftUI -o "$check_dir/check-native-layout"
"$check_dir/check-native-layout"
