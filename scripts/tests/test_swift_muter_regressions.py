import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "swift_muter_regressions", Path(__file__).parents[1] / "swift-muter-regressions.py"
)
REGRESSIONS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REGRESSIONS)


class RequiredMuterRegressionTests(unittest.TestCase):
    def test_missing_swift_fails_before_loading_tests(self):
        with mock.patch.object(REGRESSIONS.shutil, "which", side_effect=[None, "/tools/muter"]):
            with self.assertRaisesRegex(ValueError, "Swift is required"):
                REGRESSIONS.validate_tools()

    def test_missing_muter_fails_before_loading_tests(self):
        with mock.patch.object(REGRESSIONS.shutil, "which", side_effect=["/tools/swift", None]):
            with self.assertRaisesRegex(ValueError, "Pinned Muter is required"):
                REGRESSIONS.validate_tools()

    def test_missing_or_stale_revision_marker_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "muter-revision.txt"
            for contents in (None, "another-revision\n"):
                with self.subTest(marker=contents):
                    if contents:
                        marker.write_text(contents, encoding="utf-8")
                    with mock.patch.object(REGRESSIONS.shutil, "which",
                                           side_effect=["/tools/swift", str(Path(directory) / "muter")]), \
                            mock.patch.object(REGRESSIONS, "load_module",
                                              return_value=SimpleNamespace(REVISION="pinned")), \
                            mock.patch.object(REGRESSIONS, "run_tool") as run:
                        with self.assertRaisesRegex(ValueError, "revision does not match"):
                            REGRESSIONS.validate_tools()
                        run.assert_not_called()

    def test_pinned_tools_must_execute_successfully(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "muter-revision.txt").write_text("pinned\n", encoding="utf-8")
            swift, muter = "/tools/swift", str(Path(directory) / "muter")
            success = subprocess.CompletedProcess([], 0, "version", "")
            failure = subprocess.CompletedProcess([], 127, "", "missing shared library")
            for outputs in ([success, success], [failure], [success, failure]):
                with self.subTest(outputs=outputs), \
                        mock.patch.object(REGRESSIONS.shutil, "which", side_effect=[swift, muter]), \
                        mock.patch.object(REGRESSIONS, "load_module",
                                          return_value=SimpleNamespace(REVISION="pinned")), \
                        mock.patch.object(REGRESSIONS, "run_tool", side_effect=outputs) as run:
                    if failure in outputs:
                        with self.assertRaisesRegex(ValueError, "missing shared library"):
                            REGRESSIONS.validate_tools()
                    else:
                        self.assertEqual(REGRESSIONS.validate_tools(), (swift, muter))
                        self.assertEqual(run.call_args_list,
                                         [mock.call([tool, "--version"], timeout=30) for tool in (swift, muter)])

    def test_required_suite_selects_both_actual_tool_tests(self):
        suite = REGRESSIONS.load_regressions()
        self.assertEqual(suite.countTestCases(), 2)
        self.assertEqual({test._testMethodName for test in suite},
                         {entry[2] for entry in REGRESSIONS.REGRESSIONS})

    def test_main_rejects_skips_missing_cases_and_failures(self):
        results = (
            (SimpleNamespace(skipped=[], testsRun=2, wasSuccessful=lambda: True), 0),
            (SimpleNamespace(skipped=[("test", "missing tool")], testsRun=2,
                             wasSuccessful=lambda: True), 1),
            (SimpleNamespace(skipped=[], testsRun=1, wasSuccessful=lambda: True), 1),
            (SimpleNamespace(skipped=[], testsRun=2, wasSuccessful=lambda: False), 1),
        )
        for result, expected in results:
            with self.subTest(result=result), mock.patch.dict(os.environ), \
                    mock.patch.object(REGRESSIONS, "validate_tools", return_value=("/tools/swift", "/tools/muter")), \
                    mock.patch.object(REGRESSIONS, "load_regressions", return_value=unittest.TestSuite()), \
                    mock.patch.object(REGRESSIONS.unittest, "TextTestRunner") as runner, \
                    mock.patch.object(REGRESSIONS.sys, "stderr", io.StringIO()):
                runner.return_value.run.return_value = result
                self.assertEqual(REGRESSIONS.main(), expected)
                self.assertEqual(os.environ["SWIFT_EXECUTABLE"], "/tools/swift")
                self.assertEqual(os.environ["SWIFT_MUTER_EXECUTABLE"], "/tools/muter")

    def test_main_reports_unavailable_tool_and_fails(self):
        with mock.patch.object(REGRESSIONS, "validate_tools", side_effect=ValueError("Swift is required")), \
                mock.patch.object(REGRESSIONS, "load_regressions") as load, \
                mock.patch.object(REGRESSIONS.sys, "stderr", io.StringIO()) as stderr:
            self.assertEqual(REGRESSIONS.main(), 1)
            self.assertIn("Swift is required", stderr.getvalue())
            load.assert_not_called()


if __name__ == "__main__":
    unittest.main()
