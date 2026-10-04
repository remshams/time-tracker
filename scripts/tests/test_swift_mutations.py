from __future__ import annotations

import copy
import importlib.util
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "swift-mutations.py"
SPEC = importlib.util.spec_from_file_location("swift_mutations", SCRIPT)
mutations = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(mutations)


class SwiftMutationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.package = self.directory / "TrackerClient"
        self.source = self.package / "Sources/TrackerClient/Session.swift"
        self.source.parent.mkdir(parents=True)
        self.source.write_text("func start() {}\n", encoding="utf-8")
        (self.package / "Package.swift").write_text("// swift-tools-version: 5.9\n", encoding="utf-8")
        (self.package / "Tests").mkdir()
        self.output = self.directory / "reports"

    def report(self, outcomes: tuple[str, ...] = ("failed", "failed"), *, package: Path | None = None) -> dict:
        package = package or self.package
        return {"totalAppliedMutationOperators": len(outcomes),
                "numberOfKilledMutants": sum(outcome in mutations.KILLED for outcome in outcomes),
                "fileReports": [{"fileName": "Session.swift", "appliedOperators": [
                    {"testSuiteOutcome": outcome, "mutationPoint": {
                        "filePath": str(package / "Sources/TrackerClient/Session.swift"),
                        "position": {"line": 1, "column": index + 1, "utf8Offset": index},
                        "mutationOperatorId": "RemoveSideEffects"}}
                    for index, outcome in enumerate(outcomes)]}]}

    def summarize(self, report: object, *, focused: bool = False, logs: Path | None = None) -> dict:
        return mutations.mutation_summary(report, self.package, [self.source], focused=focused, logs=logs)

    def test_test_failures_count_as_killed(self) -> None:
        summary = self.summarize(self.report())
        mutations.check_mutations(summary)
        self.assertEqual(summary["killed_mutants"], 2)
        self.assertEqual(summary["scope"], "full-package")

    def test_runtime_error_without_verified_test_crash_fails(self) -> None:
        summary = self.summarize(self.report(("failed", "runtimeError")))
        self.assertEqual(summary["reported_killed_mutants"], 2)
        self.assertEqual(summary["killed_mutants"], 1)
        with self.assertRaisesRegex(mutations.MutationError, "runtimeError"):
            mutations.check_mutations(summary)

    def test_runtime_error_requires_crash_evidence_after_test_start_in_matching_log(self) -> None:
        logs = self.directory / "logs"
        logs.mkdir()
        log = logs / "Session.swift_RemoveSideEffects_0_1_1.log"
        report = self.report(("runtimeError",))
        for text in ("error: emit-module command failed\n", "*** Signal 11: Backtracing\n",
                     "Test Suite 'All tests' started\nFatal error: missing resource\n",
                     "*** Signal 11: Backtracing\nTest Case 'SessionTests.testStart' started.\n",
                     "Test Case 'SessionTests.testStart' started.\ncompiler crashed\n"):
            log.write_text(text, encoding="utf-8")
            with self.subTest(text=text), self.assertRaisesRegex(mutations.MutationError, "runtimeError"):
                mutations.check_mutations(self.summarize(report, logs=logs))
        log.write_text("Test Case 'SessionTests.testStart' started at 2026-10-04\n"
                       "*** Signal 11: Backtracing\n", encoding="utf-8")
        mutations.check_mutations(self.summarize(report, logs=logs))
        log.write_text("error: Exited with unexpected signal code 4\n"
                       "Test Case 'SessionTests.testStart' started.\nFatal error: invalid state\n", encoding="utf-8")
        mutations.check_mutations(self.summarize(report, logs=logs))
        log.write_text("error: Exited with unexpected signal code 4\n"
                       "Test Case 'SessionTests.testStart' started.\n", encoding="utf-8")
        with self.assertRaisesRegex(mutations.MutationError, "runtimeError"):
            mutations.check_mutations(self.summarize(report, logs=logs))
        log.write_text("Test Case 'SessionTests.testStart' started.\nFatal error: invalid state\n", encoding="utf-8")
        log.rename(logs / "Wrong.swift_RemoveSideEffects_0_1_1.log")
        with self.assertRaisesRegex(mutations.MutationError, "runtimeError"):
            mutations.check_mutations(self.summarize(report, logs=logs))
        observer_log = logs / "RemoveSideEffects @ Session.swift-1-1.log"
        observer_log.write_text("Test Case 'SessionTests.testStart' started.\nFatal error: invalid state\n", encoding="utf-8")
        mutations.check_mutations(self.summarize(report, logs=logs))

    def test_swift_executable_symlink_keeps_driver_name(self) -> None:
        driver = self.directory / "swift-driver"
        driver.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        driver.chmod(0o755)
        swift = self.directory / "swift"
        swift.symlink_to(driver)
        self.assertEqual(mutations.executable(str(swift)), str(swift))

    def test_each_unresolved_outcome_fails_even_with_high_overall_score(self) -> None:
        for outcome in ("passed", "timeout", "noCoverage", "buildError"):
            with self.subTest(outcome=outcome):
                summary = self.summarize(self.report(tuple(["failed"] * 100 + [outcome])))
                with self.assertRaisesRegex(mutations.MutationError, outcome):
                    mutations.check_mutations(summary)

    def test_focused_success_is_not_reported_as_full_package(self) -> None:
        summary = self.summarize(self.report(), focused=True)
        self.assertEqual(summary["scope"], "focused")
        self.assertEqual(summary["files"], ["Sources/TrackerClient/Session.swift"])

    def test_mutated_copy_source_paths_are_accepted(self) -> None:
        self.summarize(self.report(package=self.package.with_name("TrackerClient_mutated")))

    def test_paths_outside_requested_production_sources_fail(self) -> None:
        for filename in ("Tests/Session.swift", "Sources/OtherModule/Session.swift",
                         "Sources/TrackerClient/Other.swift", "../TrackerClient_other/Sources/TrackerClient/Session.swift"):
            report = self.report()
            report["fileReports"][0]["appliedOperators"][0]["mutationPoint"]["filePath"] = str(self.package / filename)
            with self.subTest(filename=filename), self.assertRaisesRegex(mutations.MutationError, "outside"):
                self.summarize(report)

    def test_relative_paths_fail(self) -> None:
        report = self.report()
        report["fileReports"][0]["appliedOperators"][0]["mutationPoint"]["filePath"] = "Sources/TrackerClient/Session.swift"
        with self.assertRaisesRegex(mutations.MutationError, "absolute"):
            self.summarize(report)

    def test_zero_missing_and_malformed_reports_fail(self) -> None:
        for report in (None, {}, self.report(()), {"totalAppliedMutationOperators": 1, "numberOfKilledMutants": 1},
                       {"totalAppliedMutationOperators": True, "numberOfKilledMutants": 1}):
            with self.subTest(report=report), self.assertRaises(mutations.MutationError):
                self.summarize(report)
        report = self.report()
        report["fileReports"] = []
        with self.assertRaisesRegex(mutations.MutationError, "no file"):
            self.summarize(report)

    def test_count_disagreements_fail(self) -> None:
        for key, value in (("numberOfKilledMutants", 1), ("totalAppliedMutationOperators", 3)):
            report = self.report()
            report[key] = value
            with self.subTest(key=key), self.assertRaisesRegex(mutations.MutationError, "disagree"):
                self.summarize(report)

    def test_duplicate_mutations_fail(self) -> None:
        report = self.report()
        applied = report["fileReports"][0]["appliedOperators"]
        applied[1] = copy.deepcopy(applied[0])
        with self.assertRaisesRegex(mutations.MutationError, "repeats"):
            self.summarize(report)

    def test_unknown_outcomes_and_invalid_metadata_fail(self) -> None:
        cases = [("testSuiteOutcome", "unrecognized"), ("testSuiteOutcome", []),
                 ("position", {"line": 0, "column": 1, "utf8Offset": 0}),
                 ("position", {"line": True, "column": 1, "utf8Offset": 0}),
                 ("position", None), ("mutationOperatorId", []), ("mutationOperatorId", "unknown")]
        for key, value in cases:
            report = self.report()
            entry = report["fileReports"][0]["appliedOperators"][0]
            if key == "testSuiteOutcome":
                entry[key] = value
            else:
                entry["mutationPoint"][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(mutations.MutationError):
                self.summarize(report)

    def test_focused_file_selection_rejects_tests_globs_and_unknown_files(self) -> None:
        for selected in (["Tests/Session.swift"], ["Sources/TrackerClient/*.swift"], [str(self.source)],
                         ["Sources/TrackerClient/Missing.swift"], [],
                         ["Sources/TrackerClient/Session.swift", "Sources/TrackerClient/Session.swift"]):
            with self.subTest(selected=selected), self.assertRaises(mutations.MutationError):
                mutations.production_files(self.package, selected)

    def test_duplicate_source_basenames_fail_even_for_focused_runs(self) -> None:
        duplicate = self.source.parent / "OtherFeature/Session.swift"
        duplicate.parent.mkdir()
        duplicate.write_text("func stop() {}\n", encoding="utf-8")
        for selected in (None, ["Sources/TrackerClient/Session.swift"]):
            with self.subTest(selected=selected), self.assertRaisesRegex(mutations.MutationError, "duplicate source file"):
                mutations.production_files(self.package, selected)

    def test_baseline_failure_removes_stale_reports_and_never_runs_muter(self) -> None:
        self.output.mkdir()
        (self.output / "report.json").write_text(json.dumps(self.report()), encoding="utf-8")
        (self.output / "summary.json").write_text("{}", encoding="utf-8")
        with patch.object(mutations, "PACKAGE", self.package), patch.object(mutations, "OUTPUT", self.output), \
                patch.object(mutations, "executable", side_effect=lambda command: command), \
                patch.object(mutations, "run", side_effect=mutations.MutationError("baseline failed")) as run:
            with self.assertRaisesRegex(mutations.MutationError, "baseline failed"):
                mutations.collect("swift", "muter")
        self.assertEqual(run.call_count, 1)
        self.assertFalse((self.output / "report.json").exists())
        self.assertFalse((self.output / "summary.json").exists())

    def test_missing_report_cannot_reuse_previous_success(self) -> None:
        self.output.mkdir()
        (self.output / "report.json").write_text(json.dumps(self.report()), encoding="utf-8")
        with patch.object(mutations, "PACKAGE", self.package), patch.object(mutations, "OUTPUT", self.output), \
                patch.object(mutations, "executable", side_effect=lambda command: command), \
                patch.object(mutations, "run", return_value=0):
            with self.assertRaisesRegex(mutations.MutationError, "valid report"):
                mutations.collect("swift", "muter")

    def test_baseline_abort_preserves_current_logs_and_removes_old_logs(self) -> None:
        old_log = self.output / "test-logs/old.log"
        old_log.parent.mkdir(parents=True)
        old_log.write_text("previous success", encoding="utf-8")

        def fake_run(arguments, package, log, **kwargs):
            path = package / "muter_logs/baseline run.log"
            path.parent.mkdir()
            path.write_text("baseline failed", encoding="utf-8")
            raise mutations.MutationError("baseline failed")

        with patch.object(mutations, "PACKAGE", self.package), patch.object(mutations, "OUTPUT", self.output), \
                patch.object(mutations, "executable", side_effect=lambda command: command), \
                patch.object(mutations, "run", side_effect=fake_run):
            with self.assertRaisesRegex(mutations.MutationError, "baseline failed"):
                mutations.collect("swift", "muter")
        self.assertFalse(old_log.exists())
        preserved = self.output / "test-logs/TrackerClient/muter_logs/baseline run.log"
        self.assertEqual(preserved.read_text(encoding="utf-8"), "baseline failed")

    def test_isolation_omits_build_cache_and_preserves_source_and_reports(self) -> None:
        cache = self.package / ".build"
        cache.mkdir()
        (cache / "sentinel").write_text("cache", encoding="utf-8")
        calls = []

        def fake_run(arguments, package, log, **kwargs):
            calls.append(arguments)
            self.assertNotEqual(package, self.package)
            self.assertFalse((package / ".build/sentinel").exists())
            self.assertTrue((package / "Tests").is_dir())
            if arguments[0] == "swift":
                module_cache = package / ".build/module-cache"
                module_cache.mkdir(parents=True)
                (module_cache / "sentinel").write_text("baseline cache", encoding="utf-8")
            if arguments[0] == "muter":
                self.assertFalse((package / ".build").exists())
                config = json.loads((package / "muter.conf.yml").read_text(encoding="utf-8"))
                self.assertEqual(config["mutationTestTimeout"], 180)
                self.assertEqual(config["arguments"], ["test", "-j", "2", "--parallel", "--num-workers", "4"])
                self.assertIn("--skip-coverage", arguments)
                self.assertIn("--skip-update-check", arguments)
                copied = package / "Sources/TrackerClient/Session.swift"
                copied.write_text("mutated", encoding="utf-8")
                (self.output / "report.json").write_text(json.dumps(self.report(package=package)), encoding="utf-8")
            return 0

        with patch.object(mutations, "PACKAGE", self.package), patch.object(mutations, "OUTPUT", self.output), \
                patch.object(mutations, "executable", side_effect=lambda command: command), \
                patch.object(mutations, "run", side_effect=fake_run):
            summary = mutations.collect("swift", "muter", ["Sources/TrackerClient/Session.swift"])
        self.assertEqual(summary["scope"], "focused")
        self.assertEqual(calls[0], ["swift", "test", "-j", "2", "--parallel", "--num-workers", "4"])
        self.assertIn("--files-to-mutate", calls[1])
        self.assertEqual(self.source.read_text(encoding="utf-8"), "func start() {}\n")
        self.assertTrue((self.output / "report.json").is_file())
        self.assertTrue((self.output / "summary.json").is_file())

    def test_muter_nonzero_exit_fails_even_with_all_killed_report(self) -> None:
        def fake_run(arguments, package, log, **kwargs):
            if arguments[0] == "muter":
                (self.output / "report.json").write_text(json.dumps(self.report(package=package)), encoding="utf-8")
                return 1
            return 0

        with patch.object(mutations, "PACKAGE", self.package), patch.object(mutations, "OUTPUT", self.output), \
                patch.object(mutations, "executable", side_effect=lambda command: command), \
                patch.object(mutations, "run", side_effect=fake_run):
            with self.assertRaisesRegex(mutations.MutationError, "exited with code 1"):
                mutations.collect("swift", "muter")

    def test_subprocess_errors_are_readable(self) -> None:
        with (self.directory / "run.log").open("w", encoding="utf-8") as log:
            with patch.object(subprocess, "Popen", side_effect=FileNotFoundError("missing")), \
                    self.assertRaisesRegex(mutations.MutationError, "Could not run"):
                mutations.run(["missing"], self.package, log)
            with self.assertRaisesRegex(mutations.MutationError, "exit code 2"):
                mutations.run([sys.executable, "-c", "raise SystemExit(2)"], self.package, log)

    def test_invalid_process_timeouts_fail_before_spawning(self) -> None:
        for timeout in (0, -1, float("nan"), float("inf")):
            with self.subTest(timeout=timeout), self.assertRaisesRegex(mutations.MutationError, "positive, finite"):
                mutations.collect("swift", "muter", timeout=timeout)

    def assert_child_stopped(self, marker: Path) -> None:
        self.assertTrue(marker.exists(), "The subprocess did not reach its child-process fixture")
        pid = int(marker.read_text(encoding="utf-8"))

        def stop_if_needed():
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

        self.addCleanup(stop_if_needed)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            result = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], text=True,
                                    stdout=subprocess.PIPE, check=False)
            state = result.stdout.strip()
            if not state or state.startswith("Z"):
                return
            time.sleep(0.02)
        self.fail(f"Child process {pid} is still running after cleanup")

    @unittest.skipUnless(os.name == "posix", "Process groups require POSIX")
    def test_timeout_kills_descendant_that_ignores_sigterm(self) -> None:
        marker = self.directory / "child.pid"
        child = ("import os, signal, sys, time; from pathlib import Path; "
                 "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                 "Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(30)")
        parent = ("import subprocess, sys, time; "
                  "subprocess.Popen([sys.executable, '-c', sys.argv[1], sys.argv[2]]); time.sleep(30)")
        with (self.directory / "timeout.log").open("w", encoding="utf-8") as log:
            with self.assertRaisesRegex(mutations.MutationError, "timed out"):
                mutations.run([sys.executable, "-c", parent, child, str(marker)], self.package, log, timeout=1)
        self.assert_child_stopped(marker)

    @unittest.skipUnless(os.name == "posix", "Process groups require POSIX")
    def test_successful_parent_cannot_leave_test_descendants_running(self) -> None:
        marker = self.directory / "child.pid"
        child = ("import os, signal, sys, time; from pathlib import Path; "
                 "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                 "Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(30)")
        parent = ("import subprocess, sys, time; from pathlib import Path; "
                  "subprocess.Popen([sys.executable, '-c', sys.argv[1], sys.argv[2]]); "
                  "deadline = time.monotonic() + 2\n"
                  "while not Path(sys.argv[2]).exists() and time.monotonic() < deadline: time.sleep(0.01)")
        with (self.directory / "success.log").open("w", encoding="utf-8") as log:
            status = mutations.run([sys.executable, "-c", parent, child, str(marker)], self.package, log, timeout=3)
        self.assertEqual(status, 0)
        self.assert_child_stopped(marker)

    def test_timeout_cannot_accept_report_written_before_the_hang(self) -> None:
        def fake_run(arguments, package, log, **kwargs):
            if arguments[0] == "muter":
                (self.output / "report.json").write_text(json.dumps(self.report(package=package)), encoding="utf-8")
                (package / "Session.swift_RemoveSideEffects_0_1_1.log").write_text("hung test", encoding="utf-8")
                (package / "baseline run").write_text("successful baseline", encoding="utf-8")
                raise mutations.MutationError("Command timed out")
            return 0

        with patch.object(mutations, "PACKAGE", self.package), patch.object(mutations, "OUTPUT", self.output), \
                patch.object(mutations, "executable", side_effect=lambda command: command), \
                patch.object(mutations, "run", side_effect=fake_run):
            with self.assertRaisesRegex(mutations.MutationError, "timed out"):
                mutations.collect("swift", "muter", timeout=42)
        self.assertFalse((self.output / "summary.json").exists())
        logs = self.output / "test-logs/TrackerClient"
        self.assertEqual((logs / "Session.swift_RemoveSideEffects_0_1_1.log").read_text(encoding="utf-8"), "hung test")
        self.assertEqual((logs / "baseline run").read_text(encoding="utf-8"), "successful baseline")


if __name__ == "__main__":
    unittest.main()
