import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from scripts import swift_muter_tools as TOOLS


class MuterToolProcessTests(unittest.TestCase):
    def test_success_and_nonzero_exit_preserve_output(self):
        for status in (0, 7):
            with self.subTest(status=status):
                result = TOOLS.run_tool([sys.executable, "-c",
                                        f'import sys; print("output"); print("error", file=sys.stderr); sys.exit({status})'])
                self.assertEqual(result.returncode, status)
                self.assertIn("output", result.stdout)
                self.assertIn("error", result.stdout)

    def test_timeout_stops_descendant_that_ignores_termination(self):
        child = ("import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                 "time.sleep(30)")
        parent = ("import signal, subprocess, sys, time; "
                  "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                  f"child = subprocess.Popen([sys.executable, '-c', {child!r}]); "
                  "print(child.pid, flush=True); time.sleep(30)")
        started = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired) as failure:
            TOOLS.run_tool([sys.executable, "-c", parent], timeout=0.2)
        self.assertLess(time.monotonic() - started, 4)
        descendant = int(failure.exception.output.strip())
        state = subprocess.run(["ps", "-p", str(descendant), "-o", "stat="],
                               capture_output=True, text=True, timeout=5).stdout.strip()
        try:
            self.assertTrue(not state or state.startswith("Z"),
                            "A timed-out compiler/test descendant kept running")
        finally:
            if state and not state.startswith("Z"):
                os.kill(descendant, signal.SIGKILL)

    def test_completed_parent_still_stops_its_descendants(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "descendant-finished"
            child = f"import pathlib, time; time.sleep(0.5); pathlib.Path({str(marker)!r}).write_text('still running')"
            parent = f"import subprocess, sys; subprocess.Popen([sys.executable, '-c', {child!r}]); print('done')"
            result = TOOLS.run_tool([sys.executable, "-c", parent])
            self.assertEqual(result.returncode, 0)
            time.sleep(0.6)
            self.assertFalse(marker.exists(), "Tool completion left a compiler/test descendant running")

    def test_interruption_also_cleans_process_group(self):
        process = mock.Mock(pid=123)
        process.wait.side_effect = [KeyboardInterrupt, None, None]
        with mock.patch.object(TOOLS.subprocess, "Popen", return_value=process), \
                mock.patch.object(TOOLS.os, "killpg") as kill:
            with self.assertRaises(KeyboardInterrupt):
                TOOLS.run_tool(["tool"])
        self.assertEqual(kill.call_args_list,
                         [mock.call(123, signal.SIGTERM), mock.call(123, signal.SIGKILL)])

    def test_missing_executable_fails_without_cleanup_of_unstarted_process(self):
        with mock.patch.object(TOOLS.os, "killpg") as kill:
            with self.assertRaises(FileNotFoundError):
                TOOLS.run_tool(["/missing/tracker-muter-regression-tool"])
            kill.assert_not_called()


if __name__ == "__main__":
    unittest.main()
