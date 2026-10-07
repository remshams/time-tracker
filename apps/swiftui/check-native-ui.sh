#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
    echo 'Native UI tests require macOS, Xcode, and a desktop session.' >&2
    exit 1
fi

app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$app_dir/../.." && pwd)
cd "$repo_dir"

cargo build --locked --package tracker-cli --bin tt-cli
target_dir=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
export TT_UI_TEST_CLI="$target_dir/debug/tt-cli"
export TEST_RUNNER_TT_UI_TEST_CLI="$TT_UI_TEST_CLI"
if [[ ! -x "$TT_UI_TEST_CLI" ]]; then
    echo "The UI test fixture CLI is not executable: $TT_UI_TEST_CLI" >&2
    exit 1
fi

mkdir -p .build
rm -rf .build/native-ui.xcresult
xcodebuild -project apps/swiftui/TimeTracker.xcodeproj -scheme TimeTracker \
    -configuration Debug -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath .build/native-ui -resultBundlePath .build/native-ui.xcresult \
    -parallel-testing-enabled NO -only-testing:TimeTrackerUITests \
    CODE_SIGNING_ALLOWED=NO test | tee .build/native-ui.log
