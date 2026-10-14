#!/usr/bin/env python3
"""Run baseline and candidate native pagination samples sequentially."""

import datetime
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import time


METHOD = "testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs"
TESTS = {
    f"TimeTrackerUITests.{suite} {METHOD}"
    for suite in ("WorklogHistoryUITests", "ServerWorklogHistoryUITests")
}
PHASES = {"fixture-setup", "launch-readiness", "traversal", "assertions", "teardown"}


def valid_seconds(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def canonical_test_name(value):
    if not isinstance(value, str):
        return None
    matches = [name for name in TESTS if name == value or name.endswith(f".{value}")]
    return matches[0] if len(matches) == 1 else None


def validate_timing_report(path):
    """Return measurement errors without hiding runner failures."""
    errors = []
    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return ["No timing report was produced."]
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        return [f"Could not read timing report: {error}"]

    if not isinstance(report, dict):
        return ["Timing report must be a JSON object."]
    tests = report.get("tests")
    if not isinstance(tests, list):
        errors.append("Timing report tests must be a list.")
        tests = []
    names = []
    for test in tests:
        if not isinstance(test, dict) or not isinstance(test.get("name"), str):
            errors.append("Each timing report test must have a string name.")
            continue
        names.append(test["name"])
    if len(tests) != 2 or set(names) != TESTS or len(names) != len(TESTS):
        errors.append("Both pagination cases must execute exactly once.")
    completed_tests = report.get("completed_tests")
    if isinstance(completed_tests, bool) or not isinstance(completed_tests, int) or completed_tests != 2:
        errors.append("Timing report must contain exactly two completed tests.")

    unfinished = report.get("unfinished_tests")
    if not isinstance(unfinished, list) or any(not isinstance(name, str) for name in unfinished):
        errors.append("Timing report unfinished_tests must be a list.")
    elif unfinished:
        errors.append("A pagination case did not finish.")

    for test in tests:
        if not isinstance(test, dict):
            errors.append("A timing report test entry must be an object.")
        else:
            if test.get("result") != "passed":
                errors.append(f"Pagination case did not pass: {test.get('name', '<unknown>')}.")
            if not valid_seconds(test.get("seconds")):
                errors.append(f"Pagination case has invalid timing: {test.get('name', '<unknown>')}.")

    phase_events = report.get("phase_events")
    if not isinstance(phase_events, list):
        errors.append("Timing report phase_events must be a list.")
        phase_events = []
    phases_by_test = {name: {} for name in TESTS}
    for event in phase_events:
        if not isinstance(event, dict):
            errors.append("A phase event must be an object.")
            continue
        test_name = canonical_test_name(event.get("test"))
        phase = event.get("phase")
        seconds = event.get("seconds")
        if (test_name is None
                or not isinstance(phase, str) or not valid_seconds(seconds)):
            errors.append("A phase event has invalid test, phase, or seconds fields.")
            continue
        phases = phases_by_test[test_name]
        phases[phase] = phases.get(phase, 0) + 1
    for test_name, phases in phases_by_test.items():
        missing = PHASES - phases.keys()
        if missing:
            errors.append(f"Missing phases for {test_name}: {sorted(missing)}")
        duplicates = sorted(phase for phase, count in phases.items() if phase in PHASES and count > 1)
        if duplicates:
            errors.append(f"Duplicate phases for {test_name}: {duplicates}")

    malformed = report.get("malformed_phase_events")
    if isinstance(malformed, bool) or not isinstance(malformed, int) or malformed != 0:
        errors.append("Malformed phase events were reported.")
    return errors


def write_metadata(path, metadata):
    temporary = path.with_name(f"{path.name}.tmp")
    temporary.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def main():
    workspace = Path.cwd()
    output = workspace / ".build/native-ui-benchmark"
    baseline_checkout = workspace / ".build/native-ui-baseline"
    binding = os.environ.get("HISTORY_BINDING", "")
    if binding not in {"index", "accessibility"}:
        raise SystemExit("HISTORY_BINDING must be index or accessibility.")
    baseline_ref = os.environ.get("BASELINE_REF", "")
    if len(baseline_ref) != 40 or any(character not in "0123456789abcdefABCDEF" for character in baseline_ref):
        raise SystemExit("BASELINE_REF must be a full 40-character commit SHA.")
    baseline_last = os.environ.get("BASELINE_LAST", "false").lower() == "true"
    output.mkdir(parents=True, exist_ok=True)

    metadata = {
        "runner_class": "xcode-27",
        "runner_name": os.environ["RUNNER_NAME"],
        "runner_arch": os.environ["RUNNER_ARCH"],
        "macos": platform.mac_ver()[0],
        "xcode": subprocess.check_output(["xcodebuild", "-version"], text=True).strip(),
        "swift": subprocess.check_output(["swift", "--version"], text=True).strip(),
        "run_id": os.environ["GITHUB_RUN_ID"],
        "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        "ref": os.environ.get("GITHUB_REF", ""),
        "samples": [],
    }
    baseline = ("baseline", baseline_checkout, "index")
    candidate = (binding, workspace, binding)
    samples = [candidate, baseline] if baseline_last else [baseline, candidate]
    statuses = []

    for label, checkout, sample_binding in samples:
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
        sample = {
            "label": label,
            "revision": revision,
            "ref": baseline_ref if label == "baseline" else os.environ.get("GITHUB_REF", ""),
            "binding": sample_binding,
            "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        }
        metadata["samples"].append(sample)
        write_metadata(output / "metadata.json", metadata)

        environment = {
            **os.environ,
            "CARGO_TARGET_DIR": str(workspace / "target"),
            "TT_UI_TEST_OUTPUT_DIR": str(output / label),
            "TT_UI_HISTORY_BINDING": sample_binding,
        }
        started = time.monotonic()
        launch_error = None
        try:
            result = subprocess.run(
                ["bash", "apps/swiftui/check-native-ui.sh", "--pagination"],
                cwd=checkout, env=environment, check=False,
            )
            exit_status = result.returncode
        except OSError as error:
            exit_status = 127
            launch_error = f"Could not launch native UI runner: {error}"
        sample["exit_status"] = exit_status
        sample["wall_seconds"] = time.monotonic() - started
        if launch_error:
            sample["launch_error"] = launch_error
        errors = validate_timing_report(output / label / "native-ui-timings.json")
        sample["measurement_errors"] = errors
        for error in errors:
            print(f"{label}: {error}", flush=True)
        if launch_error:
            print(f"{label}: {launch_error}", flush=True)
        statuses.append(exit_status != 0 or bool(errors))
        write_metadata(output / "metadata.json", metadata)

    return 1 if any(statuses) else 0


if __name__ == "__main__":
    sys.exit(main())
