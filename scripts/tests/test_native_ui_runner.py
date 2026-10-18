"""Exercise native UI runner deadlines and process cleanup with real children."""

import importlib.util
from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "run-native-ui.py"


class NativeUIRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        spec = importlib.util.spec_from_file_location("native_ui_runner", SCRIPT)
        self.runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.runner)

    def run_command(self, source, *, idle_timeout=1, timeout=3):
        output = io.StringIO()
        result = self.runner.run(
            [sys.executable, "-u", "-c", source], self.directory / "native-ui.log",
            idle_timeout=idle_timeout, timeout=timeout, output=output,
            diagnose=lambda *_: None,
        )
        return result, output.getvalue()

    def test_success_and_failure_stream_stdout_and_stderr_without_changing_status(self):
        for status in (0, 7):
            with self.subTest(status=status):
                result, output = self.run_command(
                    f'import sys; print("test output"); print("test error", file=sys.stderr); sys.exit({status})'
                )
                self.assertEqual(result, status)
                self.assertIn("test output", output)
                self.assertIn("test error", output)
                self.assertEqual((self.directory / "native-ui.log").read_text(), output)

    def test_accessibility_hang_fails_instead_of_waiting_for_the_ci_job_limit(self):
        source = ('import time; '
                  'print("Test Case MenuBarUITests.testTodayRow started."); '
                  'print("Checking existence of Window MenuBarItem"); time.sleep(30)')
        started = time.monotonic()
        result, output = self.run_command(source, idle_timeout=0.2)
        self.assertEqual(result, 124)
        self.assertLess(time.monotonic() - started, 4)
        self.assertIn("no output", output)
        self.assertIn("Window MenuBarItem", (self.directory / "native-ui.log").read_text())

    def test_continuous_output_cannot_bypass_the_total_deadline(self):
        source = 'import time\nwhile True:\n print("Retrying accessibility request", flush=True)\n time.sleep(0.02)'
        result, output = self.run_command(source, idle_timeout=1, timeout=0.2)
        self.assertEqual(result, 124)
        self.assertIn("total execution", output)

    def test_output_progress_resets_the_idle_deadline(self):
        source = 'import time\nfor _ in range(4):\n print("Test progress", flush=True)\n time.sleep(0.1)'
        result, output = self.run_command(source, idle_timeout=0.3)
        self.assertEqual(result, 0)
        self.assertEqual(output.count("Test progress"), 4)

    def test_timeout_kills_descendants_even_when_the_parent_ignores_termination(self):
        marker = self.directory / "child-finished"
        child = ('import pathlib, signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); '
                 f'time.sleep(3); pathlib.Path({str(marker)!r}).write_text("still running")')
        parent = ('import signal, subprocess, sys, time; '
                  'signal.signal(signal.SIGTERM, signal.SIG_IGN); '
                  f'child = subprocess.Popen([sys.executable, "-c", {child!r}]); '
                  'print(child.pid, flush=True); time.sleep(30)')
        result, output = self.run_command(parent, idle_timeout=0.2)
        self.assertEqual(result, 124)
        pid = int(output.splitlines()[0])
        state = subprocess.run(["ps", "-p", str(pid), "-o", "stat="],
                               capture_output=True, text=True, timeout=5).stdout.strip()
        self.assertTrue(not state or state.startswith("Z"), "The stalled UI child kept running")
        self.assertFalse(marker.exists())

    def test_completed_parent_does_not_leave_a_descendant_holding_output_open(self):
        marker = self.directory / "child-finished"
        child = f'import pathlib, time; time.sleep(0.5); pathlib.Path({str(marker)!r}).write_text("still running")'
        parent = f'import subprocess, sys; subprocess.Popen([sys.executable, "-c", {child!r}]); print("done")'
        result, _ = self.run_command(parent)
        self.assertEqual(result, 0)
        time.sleep(0.6)
        self.assertFalse(marker.exists())

    def test_invalid_deadlines_fail_before_starting_a_process(self):
        for value in (0, -1, float("inf"), float("nan")):
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.run_command('print("must not start")', idle_timeout=value)

    def test_ci_termination_stops_the_running_test_process(self):
        log = self.directory / "native-ui.log"
        wrapper = subprocess.Popen(
            [sys.executable, str(SCRIPT), "--log", str(log), "--",
             sys.executable, "-u", "-c", 'import os, time; print(os.getpid()); time.sleep(30)'],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        try:
            deadline = time.monotonic() + 5
            while not log.exists() or not log.read_text().strip():
                self.assertIsNone(wrapper.poll())
                self.assertLess(time.monotonic(), deadline, "The fixture never started")
                time.sleep(0.01)
            pid = int(log.read_text().strip())
            wrapper.send_signal(signal.SIGTERM)
            self.assertEqual(wrapper.wait(timeout=5), 143)
            state = subprocess.run(["ps", "-p", str(pid), "-o", "stat="],
                                   capture_output=True, text=True, timeout=5).stdout.strip()
            self.assertTrue(not state or state.startswith("Z"), "CI cancellation left the UI process running")
        finally:
            if wrapper.poll() is None:
                wrapper.send_signal(signal.SIGTERM)
                wrapper.wait(timeout=5)

    def delayed_diagnostic(self, command, **keywords):
        source = 'import time; time.sleep(0.04)'
        if command[0] == "pgrep":
            source += '; print("101\\n102\\n103")'
        if command[0] == "sample":
            source = 'import time; time.sleep(0.15)'
        return self.real_run([sys.executable, "-c", source], **keywords)

    def diagnostic_budget(self):
        self.real_run = subprocess.run
        self.enterContext(mock.patch.object(self.runner, "DIAGNOSTICS_TIMEOUT", 0.35, create=True))
        self.enterContext(mock.patch.object(self.runner.sys, "platform", "darwin"))
        return self.enterContext(mock.patch.object(self.runner.subprocess, "run", side_effect=self.delayed_diagnostic))

    def test_diagnostics_share_one_deadline_across_process_listing_and_samples(self):
        commands = self.diagnostic_budget()
        started = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired):
            self.runner.collect_diagnostics(self.directory / "hang", 100)
        self.assertLess(time.monotonic() - started, 1)
        self.assertTrue(commands.call_args_list)
        for call in commands.call_args_list:
            self.assertGreater(call.kwargs["timeout"], 0)
            self.assertLessEqual(call.kwargs["timeout"], 0.35)

    def test_diagnostic_deadline_still_returns_timeout_and_writes_timing_report(self):
        self.diagnostic_budget()
        log = self.directory / "native-ui.log"
        output = io.StringIO()
        arguments = [str(SCRIPT), "--log", str(log), "--idle-timeout", "0.1", "--",
                     sys.executable, "-u", "-c", 'import time; print("Window lookup stalled"); time.sleep(30)']
        with mock.patch.object(self.runner.sys, "argv", arguments), \
                mock.patch.object(self.runner.signal, "signal"), redirect_stdout(output):
            self.assertEqual(self.runner.main(), 124)
        self.assertIn("Could not finish native UI diagnostics", output.getvalue())
        report = json.loads((self.directory / "native-ui-timings.json").read_text())
        self.assertEqual(report["exit_status"], 124)

    def test_shell_runner_preserves_group_selection_and_xcode_failure_with_execution_limits(self):
        repository = self.directory / "repository"
        scripts = repository / "scripts"
        application = repository / "apps/swiftui"
        scripts.mkdir(parents=True)
        (application / "Tests/UITests/Support").mkdir(parents=True)
        (application / "Tests/UITests/Support/controlled_proxy.py").touch()
        shutil.copy(SCRIPT, scripts)
        shell = SCRIPT.parents[1] / "apps/swiftui/check-native-ui.sh"
        shutil.copy(shell, application)
        tools = self.directory / "tools"
        tools.mkdir()
        target = self.directory / "target/debug"
        target.mkdir(parents=True)
        for name in ("tt-server", "tt-cli"):
            binary = target / name
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
        commands = {
            "uname": '#!/bin/sh\nif [ "$1" = -s ]; then echo Darwin; else echo arm64; fi\n',
            "cargo": f'#!{sys.executable}\nimport json, sys\n'
                     f'if sys.argv[1] == "metadata": print(json.dumps({{"target_directory": {str(target.parent)!r}}}))\n',
            "xcodebuild": f'#!{sys.executable}\nimport json, os, sys\n'
                          'print(json.dumps(sys.argv[1:]))\n'
                          'print(json.dumps({key: os.environ.get(key) for key in '
                          '("TT_UI_HISTORY_BINDING", "TEST_RUNNER_TT_UI_HISTORY_BINDING")}))\n'
                          'sys.exit(7)\n',
        }
        for name, source in commands.items():
            path = tools / name
            path.write_text(source)
            path.chmod(0o755)
        output_directory = self.directory / "benchmark" / "run-1"
        result = subprocess.run(["bash", str(application / "check-native-ui.sh"), "--group", "mac"],
                                env={**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                                     "TT_UI_TEST_OUTPUT_DIR": str(output_directory),
                                     "TT_UI_HISTORY_BINDING": "accessibility"},
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 7, result.stderr)
        arguments = json.loads(result.stdout.splitlines()[0])
        self.assertEqual([argument for argument in arguments if argument.startswith("-only-testing:")],
                         [f"-only-testing:TimeTrackerUITests/{suite}" for suite in
                          ("MenuBarUITests", "ServerMenuBarUITests", "WindowRoutingUITests",
                           "ServerWindowRoutingUITests", "LifecycleUITests", "ServerLifecycleUITests")])
        for flag, value in [("-test-timeouts-enabled", "YES"),
                            ("-default-test-execution-time-allowance", "360"),
                            ("-maximum-test-execution-time-allowance", "360")]:
            self.assertEqual(arguments[arguments.index(flag) + 1], value)
        self.assertEqual(arguments[arguments.index("-derivedDataPath") + 1], ".build/native-debug")
        self.assertEqual(arguments[arguments.index("-resultBundlePath") + 1],
                         str(output_directory / "native-ui.xcresult"))
        forwarded_environment = json.loads(result.stdout.splitlines()[1])
        self.assertEqual(forwarded_environment, {"TT_UI_HISTORY_BINDING": "accessibility",
                                                 "TEST_RUNNER_TT_UI_HISTORY_BINDING": "accessibility"})
        report = json.loads((output_directory / "native-ui-timings.json").read_text())
        self.assertEqual(report["exit_status"], 7)
        self.assertEqual(report["timeout_seconds"], 1800)
        result = subprocess.run(["bash", str(application / "check-native-ui.sh")],
                                env={**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"]},
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 7, result.stderr)
        arguments = json.loads(result.stdout.splitlines()[0])
        self.assertIn("-only-testing:TimeTrackerUITests", arguments)
        report = json.loads((repository / ".build/native-ui-timings.json").read_text())
        self.assertEqual(report["timeout_seconds"], 7200)

        pagination_output = self.directory / "benchmark" / "pagination"
        result = subprocess.run(["bash", str(application / "check-native-ui.sh"), "--pagination"],
                                env={**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                                     "TT_UI_TEST_OUTPUT_DIR": str(pagination_output)},
                                capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 7, result.stderr)
        arguments = json.loads(result.stdout.splitlines()[0])
        self.assertEqual([argument for argument in arguments if argument.startswith("-only-testing:")], [
            "-only-testing:TimeTrackerUITests/WorklogHistoryUITests/"
            "testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs",
            "-only-testing:TimeTrackerUITests/ServerWorklogHistoryUITests/"
            "testHistoryPaginationLoadsMoreThanFiftyWithoutMissingOrDuplicateIDs",
        ])
        self.assertEqual(arguments[arguments.index("-resultBundlePath") + 1],
                         str(pagination_output / "native-ui.xcresult"))
        for flag, value in [("-default-test-execution-time-allowance", "360"),
                            ("-maximum-test-execution-time-allowance", "360")]:
            self.assertEqual(arguments[arguments.index(flag) + 1], value)
        report = json.loads((pagination_output / "native-ui-timings.json").read_text())
        self.assertEqual(report["timeout_seconds"], 1440)

    def test_timing_report_keeps_completed_and_stalled_test_names(self):
        log = self.directory / "native-ui.log"
        log.write_text("Test Case '-[TimeTrackerUITests.MenuBarUITests testFirst]' started.\n"
                       "Test Case '-[TimeTrackerUITests.MenuBarUITests testFirst]' passed (15.218 seconds).\n"
                       "Test Case '-[TimeTrackerUITests.MenuBarUITests testSecond]' started.\n")
        report = self.runner.summarize(log)
        self.assertEqual(report["completed_tests"], 1)
        self.assertEqual(report["test_seconds"], 15.218)
        self.assertEqual(report["unfinished_tests"], ["TimeTrackerUITests.MenuBarUITests testSecond"])

    def test_phase_timing_summary_ignores_malformed_records_and_keeps_failed_tests(self):
        log = self.directory / "native-ui.log"
        log.write_text(
            "Test Case '-[TimeTrackerUITests.WorklogHistoryUITests testLocal]' started.\n"
            "Test Case '-[TimeTrackerUITests.WorklogHistoryUITests testLocal]' failed (12.500 seconds).\n"
            "Test Case '-[TimeTrackerUITests.WorklogHistoryUITests testServer]' started.\n"
            'TT_UI_PHASE {"test":"TimeTrackerUITests.WorklogHistoryUITests testLocal",'
            '"phase":"fixture","seconds":1.25}\n'
            'TT_UI_PHASE {"test":"TimeTrackerUITests.WorklogHistoryUITests testLocal",'
            '"phase":"fixture","seconds":2}\n'
            'TT_UI_PHASE {"test":"testLocal","phase":"fixture","seconds":true}\n'
            'TT_UI_PHASE {"test":"testLocal","phase":"fixture","seconds":-1}\n'
            'TT_UI_PHASE {"test":"testLocal","phase":"fixture","seconds":NaN}\n'
            'TT_UI_PHASE {"test":3,"phase":"fixture","seconds":1}\n'
            'TT_UI_PHASE {"test":"testLocal","phase":null,"seconds":1}\n'
            'TT_UI_PHASE not-json\n'
        )
        report = self.runner.summarize(log)
        self.assertEqual(report["tests"], [{"name": "TimeTrackerUITests.WorklogHistoryUITests testLocal",
                                            "result": "failed", "seconds": 12.5}])
        self.assertEqual(report["unfinished_tests"], ["TimeTrackerUITests.WorklogHistoryUITests testServer"])
        self.assertEqual(report["slowest_tests"], report["tests"])
        self.assertEqual(report["phase_events"], [
            {"test": "TimeTrackerUITests.WorklogHistoryUITests testLocal", "phase": "fixture", "seconds": 1.25},
            {"test": "TimeTrackerUITests.WorklogHistoryUITests testLocal", "phase": "fixture", "seconds": 2},
        ])
        self.assertEqual(report["phase_aggregates"], [{
            "test": "TimeTrackerUITests.WorklogHistoryUITests testLocal", "phase": "fixture", "count": 2,
            "total_seconds": 3.25, "max_seconds": 2, "mean_seconds": 1.625,
        }])
        self.assertEqual(report["malformed_phase_events"], 6)


if __name__ == "__main__":
    unittest.main()
