#!/usr/bin/env python3
"""Run TrackerClient unit tests and check source line coverage."""

from __future__ import annotations

import argparse
import json
import math
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "apps/swiftui/TrackerClient"
OUTPUT = ROOT / ".build/swift-coverage"


class CoverageError(Exception):
    pass


def source_files(package: Path) -> list[Path]:
    sources = package / "Sources/TrackerClient"
    files = sorted(path.resolve() for path in sources.rglob("*.swift"))
    if not files:
        raise CoverageError(f"No TrackerClient Swift sources found in {sources}")
    if any(not path.is_relative_to(sources.resolve()) for path in files):
        raise CoverageError("TrackerClient sources must stay inside Sources/TrackerClient")
    return files


def coverage_summary(report: object, expected: list[Path], declaration_only: list[Path] | None = None) -> dict:
    """Check LLVM's report scope and count executable source lines."""
    if not isinstance(report, dict) or report.get("type") != "llvm.coverage.json.export":
        raise CoverageError("Expected an LLVM coverage JSON export")
    data = report.get("data")
    if not isinstance(data, list) or not data:
        raise CoverageError("Coverage report has no data")
    expected_paths = {path.resolve() for path in expected}
    found: dict[Path, dict] = {}
    for section in data:
        if not isinstance(section, dict) or not isinstance(section.get("files"), list):
            raise CoverageError("Coverage report has malformed file data")
        for entry in section["files"]:
            if not isinstance(entry, dict) or not isinstance(entry.get("filename"), str):
                raise CoverageError("Coverage report has a malformed filename")
            path = Path(entry["filename"]).resolve()
            if path not in expected_paths:
                raise CoverageError(f"Coverage report includes a file outside TrackerClient sources: {path}")
            if path in found:
                raise CoverageError(f"Coverage report repeats a source file: {path}")
            try:
                lines = entry["summary"]["lines"]
                count, covered = lines["count"], lines["covered"]
            except (KeyError, TypeError):
                raise CoverageError(f"Coverage report has no line counts for {path}") from None
            if (type(count) is not int or type(covered) is not int
                    or count < 0 or covered < 0 or covered > count):
                raise CoverageError(f"Coverage report has invalid line counts for {path}")
            found[path] = {"path": str(path), "lines": count, "covered_lines": covered}
    declared_paths = {path.resolve() for path in declaration_only or []}
    if not declared_paths <= expected_paths:
        raise CoverageError("Declaration-only sources must belong to TrackerClient")
    for path in declared_paths - found.keys():
        found[path] = {"path": str(path), "lines": 0, "covered_lines": 0,
                       "declaration_only": True}
    missing = expected_paths - found.keys()
    if missing:
        raise CoverageError("Coverage report is missing sources: " + ", ".join(str(path) for path in sorted(missing)))
    count = sum(entry["lines"] for entry in found.values())
    covered = sum(entry["covered_lines"] for entry in found.values())
    if not count:
        raise CoverageError("Coverage report has no executable source lines")
    return {"lines": count, "covered_lines": covered, "line_coverage": covered * 100 / count,
            "files": [found[path] for path in sorted(found)]}


def check_threshold(summary: dict, minimum: float) -> None:
    if not math.isfinite(minimum) or not 0 <= minimum <= 100:
        raise CoverageError("Minimum line coverage must be between 0 and 100")
    if summary["line_coverage"] < minimum:
        raise CoverageError(f"TrackerClient line coverage {summary['line_coverage']:.2f}% is below {minimum:g}%")


def require_fresh(path: Path, started_ns: int) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        raise CoverageError(f"Coverage artifact is missing or empty: {path}")
    if path.stat().st_mtime_ns < started_ns:
        raise CoverageError(f"Coverage artifact is stale: {path}")


def require_protocol_only(tree: str) -> None:
    """Allow the protocol contract file only while the compiler finds no implementation."""
    top_level = re.findall(r"^  \((\w+)\b", tree, re.MULTILINE)
    nodes = re.findall(r"\((\w+)\b", tree)
    if (not tree.startswith("(source_file ") or "protocol" not in top_level
            or any(node not in {"import_decl", "protocol"} for node in top_level)
            or "brace_stmt" in nodes or any(node.endswith("_expr") for node in nodes)):
        raise CoverageError("Contracts/Dependencies.swift must contain only protocol requirements to omit line coverage")


def require_compiled_sources(source_list: Path, expected: list[Path]) -> None:
    if not source_list.is_file():
        raise CoverageError(f"SwiftPM compiled source list is missing: {source_list}")
    compiled = {Path(line).resolve() for line in source_list.read_text(encoding="utf-8").splitlines() if line}
    if compiled != set(expected):
        raise CoverageError("SwiftPM compiled sources do not match the TrackerClient source inventory")


def run(arguments: list[str], *, capture: bool = False) -> str:
    try:
        result = subprocess.run(arguments, cwd=ROOT, check=True, text=True,
                                stdout=subprocess.PIPE if capture else None)
    except FileNotFoundError:
        raise CoverageError(f"Command not found: {arguments[0]}") from None
    except subprocess.CalledProcessError as error:
        raise CoverageError(f"Command failed with exit code {error.returncode}: {' '.join(arguments)}") from None
    return result.stdout or ""


def find_llvm_cov(swift: str, explicit: str | None) -> str:
    if explicit:
        return explicit
    swift_path = shutil.which(swift)
    if swift_path:
        sibling = Path(swift_path).resolve().with_name("llvm-cov")
        if sibling.is_file():
            return str(sibling)
    if sys.platform == "darwin":
        return run(["xcrun", "--find", "llvm-cov"], capture=True).strip()
    raise CoverageError("Cannot find llvm-cov beside Swift. Pass --llvm-cov with the tool from your Swift toolchain.")


def test_binary(binary_directory: Path) -> Path:
    candidates = list(binary_directory.glob("*PackageTests.xctest"))
    if len(candidates) != 1:
        raise CoverageError(f"Expected one SwiftPM XCTest binary in {binary_directory}")
    binary = candidates[0]
    if binary.is_dir():
        binary = binary / "Contents/MacOS" / binary.stem
    if not binary.is_file():
        raise CoverageError(f"SwiftPM test executable is missing: {binary}")
    return binary


def invalidate_reports() -> None:
    for name in ("coverage.json", "lcov.info", "summary.json", "html"):
        artifact = OUTPUT / name
        if artifact.is_dir():
            shutil.rmtree(artifact)
        elif artifact.exists():
            artifact.unlink()


def collect(swift: str, llvm_cov: str, minimum: float) -> dict:
    invalidate_reports()
    expected = source_files(PACKAGE)
    check_threshold({"line_coverage": 100}, minimum)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    build_arguments = ["--package-path", str(PACKAGE), "--scratch-path", str(OUTPUT / "build")]
    build_arguments += ["--cache-path", str(OUTPUT / "cache"), "--config-path", str(OUTPUT / "config"),
                        "--security-path", str(OUTPUT / "security")]
    binary_directory = Path(run([swift, "build", *build_arguments, "--show-bin-path"], capture=True).strip())
    profile_directory = binary_directory / "codecov"
    if profile_directory.exists():
        shutil.rmtree(profile_directory)
    started_ns = time.time_ns()
    run([swift, "test", *build_arguments, "--enable-code-coverage"])
    profile = profile_directory / "default.profdata"
    require_fresh(profile, started_ns)
    require_compiled_sources(binary_directory / "TrackerClient.build/sources", expected)
    binary = test_binary(binary_directory)
    common = [str(binary), f"-instr-profile={profile}", *[str(path) for path in expected]]
    exported = run([llvm_cov, "export", "-skip-functions", *common], capture=True)
    try:
        report = json.loads(exported)
    except json.JSONDecodeError as error:
        raise CoverageError(f"llvm-cov returned invalid JSON: {error}") from None
    declarations = PACKAGE / "Sources/TrackerClient/Contracts/Dependencies.swift"
    declaration_only = [declarations] if declarations in expected else []
    summary = coverage_summary(report, expected, declaration_only)
    if any(entry.get("declaration_only") for entry in summary["files"]):
        tree = run([swift, "-frontend", "-dump-parse", str(declarations)], capture=True)
        require_protocol_only(tree)
    (OUTPUT / "coverage.json").write_text(exported, encoding="utf-8")
    lcov = run([llvm_cov, "export", "-format=lcov", *common], capture=True)
    if not lcov.strip():
        raise CoverageError("llvm-cov returned an empty LCOV report")
    (OUTPUT / "lcov.info").write_text(lcov, encoding="utf-8")
    run([llvm_cov, "show", "-format=html", f"-output-dir={OUTPUT / 'html'}", *common])
    summary["minimum_line_coverage"] = minimum
    (OUTPUT / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"TrackerClient line coverage: {summary['line_coverage']:.2f}% "
          f"({summary['covered_lines']}/{summary['lines']} executable lines)")
    print(f"Reports: {OUTPUT}")
    check_threshold(summary, minimum)
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--minimum-line-coverage", type=float, default=95,
                        help="Required executable source line coverage percentage, default 95")
    parser.add_argument("--swift", default="swift", help="Swift executable from the selected toolchain")
    parser.add_argument("--llvm-cov", help="llvm-cov executable from the same toolchain as Swift")
    arguments = parser.parse_args()
    try:
        invalidate_reports()
        collect(arguments.swift, find_llvm_cov(arguments.swift, arguments.llvm_cov),
                arguments.minimum_line_coverage)
    except CoverageError as error:
        print(f"Swift coverage failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
