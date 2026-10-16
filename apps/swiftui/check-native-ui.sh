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
cargo build --locked --package tracker-tui --bin tt
target_dir=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
export TT_UI_TEST_CLI="$target_dir/debug/tt-cli"
export TEST_RUNNER_TT_UI_TEST_CLI="$TT_UI_TEST_CLI"
export TT_UI_TEST_SERVER="$target_dir/debug/tt"
export TEST_RUNNER_TT_UI_TEST_SERVER="$TT_UI_TEST_SERVER"
export TT_UI_TEST_PROXY="$app_dir/Tests/UITests/Support/controlled_proxy.py"
export TEST_RUNNER_TT_UI_TEST_PROXY="$TT_UI_TEST_PROXY"
export TT_UI_TEST_PYTHON="$(command -v python3)"
export TEST_RUNNER_TT_UI_TEST_PYTHON="$TT_UI_TEST_PYTHON"
if [[ ${TT_UI_HISTORY_BINDING+x} ]]; then
    export TEST_RUNNER_TT_UI_HISTORY_BINDING="$TT_UI_HISTORY_BINDING"
fi
if [[ ! -x "$TT_UI_TEST_CLI" ]]; then
    echo "The UI test fixture CLI is not executable: $TT_UI_TEST_CLI" >&2
    exit 1
fi
if [[ ! -x "$TT_UI_TEST_SERVER" || ! -f "$TT_UI_TEST_PROXY" ]]; then
    echo 'The native UI server or controlled proxy is missing.' >&2
    exit 1
fi

output_dir=${TT_UI_TEST_OUTPUT_DIR:-.build}
mkdir -p "$output_dir"
rm -rf "$output_dir/native-ui.xcresult"
test_selection=(-only-testing:TimeTrackerUITests)
execution_timeout=7200
if [[ "${1:-}" == --smoke ]]; then
    execution_timeout=1440
    test_selection=(-only-testing:TimeTrackerUITests/SmokeUITests)
elif [[ "${1:-}" == --group && $# == 2 ]]; then
    execution_timeout=1440
    case "$2" in
        core) suites=(SmokeUITests TaskCatalogUITests ServerTaskCatalogUITests TaskCreationUITests ServerTaskCreationUITests TrackingUITests ServerTrackingUITests WorklogHistoryUITests ServerWorklogHistoryUITests) ;;
        tasks) suites=(TaskRenameUITests ServerTaskRenameUITests TaskArchivingUITests ServerTaskArchivingUITests BulkTaskArchivingUITests BulkTaskArchivingExtendedUITests ServerBulkTaskArchivingUITests) ;;
        worklogs) suites=(WorklogCorrectionUITests ServerWorklogCorrectionUITests WorklogMoveUITests ServerWorklogMoveUITests
            WorklogDeletionUITests ServerWorklogDeletionUITests) ;;
        server) suites=(ConnectionUITests ServerRecoveryUITests) ;;
        menu) suites=(MenuBarUITests ServerMenuBarUITests MenuSourceUITests SettingsUITests) ;;
        mac) suites=(WindowRoutingUITests DailyTotalsUITests ServerDailyTotalsUITests LifecycleUITests ServerLifecycleUITests AppearanceUITests) ;;
        *) echo "Unknown native UI group: $2" >&2; exit 2 ;;
    esac
    test_selection=()
    for suite in "${suites[@]}"; do
        test_selection+=("-only-testing:TimeTrackerUITests/$suite")
    done
elif [[ "${1:-}" == --pagination && $# == 1 ]]; then
    execution_timeout=1440
    test_selection=(
        -only-testing:TimeTrackerUITests/WorklogHistoryUITests/testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs
        -only-testing:TimeTrackerUITests/ServerWorklogHistoryUITests/testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs
    )
elif (( $# != 0 )); then
    echo 'Usage: check-native-ui.sh [--smoke | --pagination | --group core|tasks|worklogs|server|menu|mac]' >&2
    exit 2
fi
python3 scripts/run-native-ui.py --log "$output_dir/native-ui.log" --timeout "$execution_timeout" -- \
    xcodebuild -project apps/swiftui/TimeTracker.xcodeproj -scheme TimeTracker \
    -configuration Debug -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath .build/native-debug -resultBundlePath "$output_dir/native-ui.xcresult" \
    -parallel-testing-enabled NO "${test_selection[@]}" \
    -test-timeouts-enabled YES -default-test-execution-time-allowance 360 \
    -maximum-test-execution-time-allowance 360 \
    CODE_SIGNING_ALLOWED=NO test
