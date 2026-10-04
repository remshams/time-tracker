from __future__ import annotations

import importlib.util
import contextlib
import io
import os
import tempfile
import time
import unittest
from unittest import mock
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "swift-coverage.py"
SPEC = importlib.util.spec_from_file_location("swift_coverage", SCRIPT)
coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage)


class SwiftCoverageTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.package = Path(self.temporary.name)
        self.sources = self.package / "Sources/TrackerClient"
        self.sources.mkdir(parents=True)
        self.first = self.sources / "Session.swift"
        self.second = self.sources / "State.swift"
        self.first.write_text("func start() {}\n", encoding="utf-8")
        self.second.write_text("func stop() {}\n", encoding="utf-8")

    def report(self, files: list[tuple[Path, int, int]]) -> dict:
        return {"type": "llvm.coverage.json.export", "data": [{"files": [
            {"filename": str(path), "summary": {"lines": {"count": count, "covered": covered}}}
            for path, count, covered in files
        ]}]}

    def test_aggregate_coverage_counts_lines_instead_of_averaging_files(self) -> None:
        summary = coverage.coverage_summary(self.report([(self.first, 90, 90), (self.second, 10, 0)]),
                                            coverage.source_files(self.package))
        self.assertEqual(summary["line_coverage"], 90)
        self.assertEqual(summary["lines"], 100)
        coverage.check_threshold(summary, 90)
        with self.assertRaisesRegex(coverage.CoverageError, "below"):
            coverage.check_threshold(summary, 90.01)

    def test_report_missing_new_production_file_fails(self) -> None:
        with self.assertRaisesRegex(coverage.CoverageError, "missing sources.*State.swift"):
            coverage.coverage_summary(self.report([(self.first, 1, 1)]), coverage.source_files(self.package))

    def test_test_and_other_module_files_cannot_inflate_coverage(self) -> None:
        for filename in ("Tests/TrackerClientTests/StateTests.swift", "Sources/OtherModule/State.swift"):
            with self.subTest(filename=filename), self.assertRaisesRegex(coverage.CoverageError, "outside"):
                coverage.coverage_summary(self.report([(self.first, 1, 0), (self.second, 1, 0),
                                                       (self.package / filename, 100, 100)]),
                                          coverage.source_files(self.package))

    def test_malformed_line_counts_fail(self) -> None:
        for count, covered in ((1, 2), (-1, 0), (1, -1), (True, 1), (1, "1")):
            with self.subTest(count=count, covered=covered), self.assertRaisesRegex(coverage.CoverageError, "invalid line"):
                coverage.coverage_summary(self.report([(self.first, count, covered)]), [self.first])

    def test_empty_or_malformed_reports_fail(self) -> None:
        for report in (None, {}, {"type": "llvm.coverage.json.export", "data": []},
                       {"type": "llvm.coverage.json.export", "data": [{}]},
                       self.report([(self.first, 0, 0)])):
            with self.subTest(report=report), self.assertRaises(coverage.CoverageError):
                coverage.coverage_summary(report, [self.first])

    def test_duplicate_source_file_fails(self) -> None:
        with self.assertRaisesRegex(coverage.CoverageError, "repeats"):
            coverage.coverage_summary(self.report([(self.first, 1, 1), (self.first, 1, 1)]), [self.first])

    def test_protocol_only_file_is_inventoried_without_executable_lines(self) -> None:
        summary = coverage.coverage_summary(self.report([(self.first, 10, 9)]),
                                            [self.first, self.second], [self.second])
        self.assertEqual(summary["line_coverage"], 90)
        self.assertEqual(summary["files"][1], {"path": str(self.second), "lines": 0,
                                              "covered_lines": 0, "declaration_only": True})

    def test_contract_file_with_coverage_counts_is_treated_as_executable(self) -> None:
        summary = coverage.coverage_summary(self.report([(self.first, 10, 9), (self.second, 5, 3)]),
                                            [self.first, self.second], [self.second])
        self.assertEqual(summary["lines"], 15)
        self.assertEqual(summary["covered_lines"], 12)
        self.assertNotIn("declaration_only", summary["files"][1])

    def test_protocol_validation_rejects_implementations_and_initializers(self) -> None:
        protocol = '(source_file "Dependencies.swift"\n  (import_decl)\n  (protocol\n    (func_decl)))'
        coverage.require_protocol_only(protocol)
        for tree in ("", protocol + "\n  (struct_decl)",
                     protocol.replace("(func_decl)", "(func_decl (brace_stmt))"),
                     protocol.replace("(func_decl)", "(func_decl (integer_literal_expr))")):
            with self.subTest(tree=tree), self.assertRaisesRegex(coverage.CoverageError, "protocol requirements"):
                coverage.require_protocol_only(tree)

    def test_compile_inventory_rejects_uncompiled_or_wrong_scope_sources(self) -> None:
        source_list = self.package / "sources"
        source_list.write_text(f"{self.first}\n{self.second}\n", encoding="utf-8")
        coverage.require_compiled_sources(source_list, [self.first, self.second])
        for text in (f"{self.first}\n", f"{self.first}\n{self.second}\n{self.package / 'Tests/Test.swift'}\n"):
            source_list.write_text(text, encoding="utf-8")
            with self.assertRaisesRegex(coverage.CoverageError, "do not match"):
                coverage.require_compiled_sources(source_list, [self.first, self.second])

    def test_stale_empty_and_missing_profiles_fail(self) -> None:
        profile = self.package / "default.profdata"
        started_ns = time.time_ns()
        with self.assertRaisesRegex(coverage.CoverageError, "missing or empty"):
            coverage.require_fresh(profile, started_ns)
        profile.touch()
        with self.assertRaisesRegex(coverage.CoverageError, "missing or empty"):
            coverage.require_fresh(profile, started_ns)
        profile.write_bytes(b"profile")
        os.utime(profile, ns=(started_ns - 2_000_000_000, started_ns - 2_000_000_000))
        with self.assertRaisesRegex(coverage.CoverageError, "stale"):
            coverage.require_fresh(profile, started_ns)
        os.utime(profile, ns=(started_ns + 1_000_000, started_ns + 1_000_000))
        coverage.require_fresh(profile, started_ns)

    def test_invalid_thresholds_fail(self) -> None:
        for minimum in (-1, 101, float("nan"), float("inf")):
            with self.subTest(minimum=minimum), self.assertRaisesRegex(coverage.CoverageError, "between 0 and 100"):
                coverage.check_threshold({"line_coverage": 100}, minimum)

    def test_linux_and_macos_test_binary_paths(self) -> None:
        directory = self.package / "debug"
        directory.mkdir()
        test = directory / "TrackerClientPackageTests.xctest"
        test.touch()
        self.assertEqual(coverage.test_binary(directory), test)
        test.unlink()
        executable = test / "Contents/MacOS/TrackerClientPackageTests"
        executable.parent.mkdir(parents=True)
        executable.touch()
        self.assertEqual(coverage.test_binary(directory), executable)

    def old_reports(self) -> Path:
        output = self.package / "output"
        output.mkdir()
        for name in ("coverage.json", "lcov.info", "summary.json"):
            (output / name).write_text("Old passing report", encoding="utf-8")
        (output / "html").mkdir()
        (output / "html/index.html").write_text("Old passing report", encoding="utf-8")
        (output / "build").mkdir()
        (output / "build/cache").write_text("Keep compiled artifacts", encoding="utf-8")
        return output

    def assert_reports_invalidated(self, output: Path) -> None:
        for name in ("coverage.json", "lcov.info", "summary.json", "html"):
            self.assertFalse((output / name).exists(), name)
        self.assertTrue((output / "build/cache").is_file())

    def test_missing_tool_invalidates_old_reports_before_preflight(self) -> None:
        output = self.old_reports()
        with mock.patch.object(coverage, "OUTPUT", output), mock.patch("sys.platform", "linux"), \
                mock.patch("sys.argv", [str(SCRIPT), "--swift", str(self.package / "missing-swift")]), \
                contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(coverage.main(), 1)
        self.assertIn("Cannot find llvm-cov", errors.getvalue())
        self.assert_reports_invalidated(output)

    def test_invalid_threshold_invalidates_old_reports_before_build(self) -> None:
        output = self.old_reports()
        with mock.patch.object(coverage, "OUTPUT", output), mock.patch.object(coverage, "PACKAGE", self.package), \
                mock.patch("sys.argv", [str(SCRIPT), "--llvm-cov", "llvm-cov", "--minimum-line-coverage", "101"]), \
                contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(coverage.main(), 1)
        self.assertIn("between 0 and 100", errors.getvalue())
        self.assert_reports_invalidated(output)

    def test_source_preflight_invalidates_old_reports_for_direct_collect_call(self) -> None:
        output = self.old_reports()
        with mock.patch.object(coverage, "OUTPUT", output), mock.patch.object(coverage, "PACKAGE", self.package / "missing"):
            with self.assertRaisesRegex(coverage.CoverageError, "No TrackerClient Swift sources"):
                coverage.collect("swift", "llvm-cov", 95)
        self.assert_reports_invalidated(output)


if __name__ == "__main__":
    unittest.main()
