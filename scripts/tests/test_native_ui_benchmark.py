"""Check benchmark report validation and sequential sample collection."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "benchmark-native-ui.py"
METHOD = "testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs"
TEST_NAMES = [
    f"TimeTrackerUITests.{suite} {METHOD}"
    for suite in ("WorklogHistoryUITests", "ServerWorklogHistoryUITests")
]
PHASES = ["fixture-setup", "launch-readiness", "traversal", "assertions", "teardown"]


class NativeUIBenchmarkTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        spec = importlib.util.spec_from_file_location("native_ui_benchmark", SCRIPT)
        self.benchmark = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.benchmark)

    def valid_report(self):
        return {
            "completed_tests": 2,
            "tests": [{"name": name, "result": "passed", "seconds": 1.0} for name in TEST_NAMES],
            "unfinished_tests": [],
            "phase_events": [
                {"test": name, "phase": phase, "seconds": 0.1}
                for name in TEST_NAMES for phase in PHASES
            ],
            "malformed_phase_events": 0,
        }

    def test_timing_validation_rejects_bad_reports_and_accepts_valid_report(self):
        report_path = self.directory / "timings.json"
        self.assertTrue(self.benchmark.validate_timing_report(report_path))
        report_path.write_text("{")
        self.assertTrue(self.benchmark.validate_timing_report(report_path))

        valid = self.valid_report()
        report_path.write_text(json.dumps(valid))
        self.assertEqual(self.benchmark.validate_timing_report(report_path), [])

        cases = []
        wrong_names = self.valid_report()
        wrong_names["tests"][0]["name"] = "Other.test"
        cases.append(wrong_names)
        duplicate_names = self.valid_report()
        duplicate_names["tests"][1]["name"] = TEST_NAMES[0]
        cases.append(duplicate_names)
        failed = self.valid_report()
        failed["tests"][0]["result"] = "failed"
        cases.append(failed)
        unfinished = self.valid_report()
        unfinished["unfinished_tests"] = [TEST_NAMES[1]]
        cases.append(unfinished)
        missing_phase = self.valid_report()
        missing_phase["phase_events"] = [
            event for event in missing_phase["phase_events"]
            if not (event["test"] == TEST_NAMES[0] and event["phase"] == "traversal")
        ]
        cases.append(missing_phase)
        phase_from_wrong_case = self.valid_report()
        phase_from_wrong_case["phase_events"][0]["test"] = "TimeTrackerUITests.Other test"
        cases.append(phase_from_wrong_case)
        duplicate_phase = self.valid_report()
        duplicate_phase["phase_events"].append(duplicate_phase["phase_events"][0])
        cases.append(duplicate_phase)
        malformed_count = self.valid_report()
        malformed_count["malformed_phase_events"] = 1
        cases.append(malformed_count)
        invalid_schema = self.valid_report()
        invalid_schema["tests"] = [{"name": []}, None]
        cases.append(invalid_schema)
        invalid_seconds = self.valid_report()
        invalid_seconds["tests"][0]["seconds"] = float("inf")
        cases.append(invalid_seconds)

        for report in cases:
            with self.subTest(report=report):
                report_path.write_text(json.dumps(report))
                self.assertTrue(self.benchmark.validate_timing_report(report_path))

    def test_xctest_phase_names_match_exact_case_and_method_with_optional_module(self):
        path = self.directory / "timings.json"
        report = self.valid_report()
        for event in report["phase_events"]:
            event["test"] = event["test"].removeprefix("TimeTrackerUITests.")
        path.write_text(json.dumps(report))
        self.assertEqual(self.benchmark.validate_timing_report(path), [])

        report["phase_events"].append(self.valid_report()["phase_events"][0])
        path.write_text(json.dumps(report))
        self.assertTrue(self.benchmark.validate_timing_report(path))

        for invalid in [METHOD, f"WorklogHistoryUITests {METHOD}Extra", None, 3]:
            with self.subTest(name=invalid):
                self.assertIsNone(self.benchmark.canonical_test_name(invalid))

        ambiguous = {f"First.WorklogHistoryUITests {METHOD}", f"Second.WorklogHistoryUITests {METHOD}"}
        with mock.patch.object(self.benchmark, "TESTS", ambiguous):
            self.assertIsNone(self.benchmark.canonical_test_name(f"WorklogHistoryUITests {METHOD}"))

    def setup_benchmark_workspace(self):
        workspace = self.directory / "workspace"
        baseline = workspace / ".build/native-ui-baseline"
        for checkout in (workspace, baseline):
            runner = checkout / "apps/swiftui/check-native-ui.sh"
            runner.parent.mkdir(parents=True, exist_ok=True)
            runner.write_text(
                """set -eu
label=${TT_UI_TEST_OUTPUT_DIR##*/}
python3 - "$TT_UI_TEST_OUTPUT_DIR/native-ui-timings.json" "$label" "$METADATA_AT_LAUNCH" <<'PYTHON'
import json
import os
import sys
from pathlib import Path

metadata = json.loads(Path(os.environ['METADATA_PATH']).read_text())
with Path(sys.argv[3]).open('a') as output:
    output.write(json.dumps(metadata) + '\\n')
Path(os.environ['ORDER_FILE']).open('a').write(sys.argv[2] + '\\n')
if os.environ.get('MISSING_REPORT_LABEL') != sys.argv[2]:
    names = [
        'TimeTrackerUITests.WorklogHistoryUITests testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs',
        'TimeTrackerUITests.ServerWorklogHistoryUITests testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs',
    ]
    phases = ['fixture-setup', 'launch-readiness', 'traversal', 'assertions', 'teardown']
    report = {
        'completed_tests': 2,
        'tests': [{'name': name, 'result': 'passed', 'seconds': 1.0} for name in names],
        'unfinished_tests': [],
        'phase_events': [{'test': name, 'phase': phase, 'seconds': 0.1}
                         for name in names for phase in phases],
        'malformed_phase_events': 0,
    }
    Path(sys.argv[1]).parent.mkdir(parents=True, exist_ok=True)
    Path(sys.argv[1]).write_text(json.dumps(report))
PYTHON
if [[ ${FAIL_LABEL:-} == "$label" ]]; then exit 9; fi
""",
                encoding="utf-8",
            )

        tools = self.directory / "tools"
        tools.mkdir()
        (tools / "git").write_text(
            "#!/bin/sh\ncase \"$PWD\" in *native-ui-baseline) echo baseline-revision;; "
            "*) echo candidate-revision;; esac\n",
            encoding="utf-8",
        )
        for command, output in (("xcodebuild", "Xcode 27.0\\nBuild version test\\n"),
                                ("swift", "Swift version test\\n")):
            (tools / command).write_text(f"#!/bin/sh\nprintf '%b' '{output}'\n", encoding="utf-8")
        for executable in tools.iterdir():
            executable.chmod(0o755)
        return workspace, tools

    def run_benchmark(self, workspace, tools, **environment):
        return subprocess.run(
            [sys.executable, str(SCRIPT)], cwd=workspace,
            env={**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                 "RUNNER_NAME": "xcode-27-test", "RUNNER_ARCH": "arm64",
                 "GITHUB_RUN_ID": "1234", "GITHUB_RUN_ATTEMPT": "2",
                 "GITHUB_REF": "refs/heads/perf", "BASELINE_REF": "a" * 40,
                 "HISTORY_BINDING": "accessibility", "BASELINE_LAST": "false", **environment},
            capture_output=True, text=True, timeout=10,
        )

    def test_samples_run_in_order_and_partial_metadata_survives_failures(self):
        workspace, tools = self.setup_benchmark_workspace()
        output = workspace / ".build/native-ui-benchmark"
        order_file = self.directory / "order.txt"
        launch_metadata = self.directory / "metadata-at-launch.jsonl"
        result = self.run_benchmark(
            workspace, tools, ORDER_FILE=str(order_file), METADATA_AT_LAUNCH=str(launch_metadata),
            METADATA_PATH=str(output / "metadata.json"),
            FAIL_LABEL="baseline", MISSING_REPORT_LABEL="accessibility",
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(order_file.read_text().splitlines(), ["baseline", "accessibility"])
        metadata = json.loads((output / "metadata.json").read_text())
        self.assertEqual(metadata["runner_class"], "xcode-27")
        self.assertEqual(metadata["run_id"], "1234")
        self.assertEqual(metadata["run_attempt"], "2")
        self.assertEqual(metadata["ref"], "refs/heads/perf")
        self.assertEqual([sample["label"] for sample in metadata["samples"]], ["baseline", "accessibility"])
        self.assertEqual([sample["revision"] for sample in metadata["samples"]],
                         ["baseline-revision", "candidate-revision"])
        self.assertEqual([sample["ref"] for sample in metadata["samples"]], ["a" * 40, "refs/heads/perf"])
        self.assertEqual([sample["exit_status"] for sample in metadata["samples"]], [9, 0])
        self.assertTrue(metadata["samples"][0]["wall_seconds"] >= 0)
        self.assertTrue(metadata["samples"][0]["measurement_errors"] == [])
        self.assertIn("No timing report", metadata["samples"][1]["measurement_errors"][0])
        snapshots = [json.loads(line) for line in launch_metadata.read_text().splitlines()]
        self.assertEqual([len(snapshot["samples"]) for snapshot in snapshots], [1, 2])
        self.assertNotIn("exit_status", snapshots[0]["samples"][0])

    def test_baseline_last_reverses_order_and_index_candidate_uses_separate_output(self):
        workspace, tools = self.setup_benchmark_workspace()
        order_file = self.directory / "order.txt"
        launch_metadata = self.directory / "metadata-at-launch.jsonl"
        result = self.run_benchmark(
            workspace, tools, HISTORY_BINDING="index", BASELINE_LAST="true",
            ORDER_FILE=str(order_file), METADATA_AT_LAUNCH=str(launch_metadata),
            METADATA_PATH=str(workspace / ".build/native-ui-benchmark/metadata.json"),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(order_file.read_text().splitlines(), ["index", "baseline"])
        metadata = json.loads((workspace / ".build/native-ui-benchmark/metadata.json").read_text())
        self.assertEqual([sample["binding"] for sample in metadata["samples"]], ["index", "index"])
        self.assertTrue(all(sample["measurement_errors"] == [] for sample in metadata["samples"]))

    def test_runner_launch_errors_are_recorded_and_do_not_skip_the_next_sample(self):
        workspace, tools = self.setup_benchmark_workspace()
        restricted_path = str(tools)
        result = self.run_benchmark(workspace, tools, PATH=restricted_path)
        self.assertEqual(result.returncode, 1)
        metadata = json.loads((workspace / ".build/native-ui-benchmark/metadata.json").read_text())
        self.assertEqual([sample["label"] for sample in metadata["samples"]], ["baseline", "accessibility"])
        for sample in metadata["samples"]:
            self.assertEqual(sample["exit_status"], 127)
            self.assertIn("Could not launch native UI runner", sample["launch_error"])
            self.assertIn("No timing report", sample["measurement_errors"][0])


if __name__ == "__main__":
    unittest.main()
