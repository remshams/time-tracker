#!/usr/bin/env python3
"""Run Muter against an isolated TrackerClient package and reject unresolved mutants."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "apps/swiftui/TrackerClient"
OUTPUT = ROOT / ".build/swift-mutations"
DEFAULT_MUTER = ROOT / ".build/swift-tools/muter"
OUTCOMES = {"passed", "failed", "runtimeError", "buildError", "timeout", "noCoverage"}
KILLED = {"failed", "runtimeError"}
OPERATORS = {"RelationalOperatorReplacement", "RemoveSideEffects", "ChangeLogicalConnector", "SwapTernary"}


class MutationError(Exception):
    pass


def executable(command: str) -> str:
    resolved = shutil.which(command)
    if not resolved:
        raise MutationError(f"Executable not found: {command}")
    return str(Path(resolved).absolute())


def production_files(package: Path, selected: list[str] | None) -> list[Path]:
    sources = (package / "Sources/TrackerClient").resolve()
    files = sorted(path.resolve() for path in sources.rglob("*.swift"))
    if not files or any(not path.is_relative_to(sources) for path in files):
        raise MutationError("TrackerClient needs Swift sources inside Sources/TrackerClient")
    names = set()
    for path in files:
        if path.name in names:
            raise MutationError(f"Muter uses source basenames as identifiers. Rename duplicate source file: {path.name}")
        names.add(path.name)
    if selected is None:
        return files
    if not selected:
        raise MutationError("A focused run needs at least one production file")
    chosen = []
    for name in selected:
        path = (package / name).resolve()
        if Path(name).is_absolute() or path not in files:
            raise MutationError(f"Expected a package-relative TrackerClient source file: {name}")
        if path in chosen:
            raise MutationError(f"Repeated production file: {name}")
        chosen.append(path)
    return sorted(chosen)


def runtime_crash_evidence(point: dict, logs: Path | None) -> bool:
    if logs is None:
        return False
    position = point["position"]
    file_name = Path(point["filePath"]).name
    operator = point["mutationOperatorId"]
    names = {f"{file_name}_{operator}_{position['utf8Offset']}_{position['line']}_{position['column']}.log",
             f"{operator} @ {file_name}-{position['line']}-{position['column']}.log"}
    for path in logs.rglob("*.log"):
        if path.name not in names:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        started = re.search(r"(?m)^(?:Test Case .+ started(?: at |\.?$)|◇ Test .+ started\.)", text)
        crashed = started and re.search(r"Fatal error:|(?:Program|Thread [0-9]+) crashed:|"
                            r"\*\*\* Signal [0-9]+:|Exited with unexpected signal code [0-9]+",
                            text[started.end():])
        if crashed:
            return True
    return False


def preserve_test_logs(package: Path, destination: Path) -> None:
    for root in (package, package.with_name(package.name + "_mutated")):
        if not root.is_dir():
            continue
        for source in root.rglob("*"):
            if source.is_file() and (source.suffix == ".log" or source.name == "baseline run"):
                target = destination / root.name / source.relative_to(root)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, target)


def mutation_summary(report: object, package: Path, expected: list[Path], *, focused: bool,
                     logs: Path | None = None) -> dict:
    if not isinstance(report, dict):
        raise MutationError("Expected a Muter JSON object")
    total = report.get("totalAppliedMutationOperators")
    killed = report.get("numberOfKilledMutants")
    if type(total) is not int or type(killed) is not int or total <= 0 or not 0 <= killed <= total:
        raise MutationError("Muter report needs positive, consistent mutant counts")
    reports = report.get("fileReports")
    if not isinstance(reports, list) or not reports:
        raise MutationError("Muter report has no file reports")
    package = package.resolve()
    allowed = {path.relative_to(package).as_posix() for path in expected}
    roots = [package, package.with_name(package.name + "_mutated")]
    counts = {outcome: 0 for outcome in sorted(OUTCOMES)}
    mutations = []
    seen = set()
    for file_report in reports:
        if (not isinstance(file_report, dict) or not isinstance(file_report.get("fileName"), str)
                or not isinstance(file_report.get("appliedOperators"), list)
                or not file_report["appliedOperators"]):
            raise MutationError("Muter report has malformed or empty file reports")
        for entry in file_report["appliedOperators"]:
            if (not isinstance(entry, dict) or not isinstance(entry.get("testSuiteOutcome"), str)
                    or entry["testSuiteOutcome"] not in OUTCOMES):
                raise MutationError("Muter report has an unknown test outcome")
            point = entry.get("mutationPoint")
            if not isinstance(point, dict) or not isinstance(point.get("filePath"), str):
                raise MutationError("Muter report has a malformed mutation point")
            path = Path(point["filePath"])
            if not path.is_absolute():
                raise MutationError(f"Mutation path must be absolute: {path}")
            path = path.resolve()
            relative = next((path.relative_to(root).as_posix() for root in roots
                             if path.is_relative_to(root)), None)
            if relative not in allowed or file_report["fileName"] != path.name:
                raise MutationError(f"Mutation report includes a file outside the requested production sources: {path}")
            position = point.get("position")
            operator = point.get("mutationOperatorId")
            if (not isinstance(position, dict) or not isinstance(operator, str) or operator not in OPERATORS
                    or any(type(position.get(key)) is not int or position[key] < minimum
                           for key, minimum in (("line", 1), ("column", 1), ("utf8Offset", 0)))):
                raise MutationError(f"Muter report has invalid mutation metadata for {relative}")
            identity = (relative, position["utf8Offset"], operator)
            if identity in seen:
                raise MutationError(f"Muter report repeats a mutation: {relative}:{position['line']}")
            seen.add(identity)
            outcome = entry["testSuiteOutcome"]
            counts[outcome] += 1
            mutations.append({"path": relative, "line": position["line"], "column": position["column"],
                              "operator": operator, "outcome": outcome,
                              "verified_kill": outcome == "failed" or
                              (outcome == "runtimeError" and runtime_crash_evidence(point, logs))})
    if len(mutations) != total or sum(counts[outcome] for outcome in KILLED) != killed:
        raise MutationError("Muter report mutant counts disagree with recorded outcomes")
    return {"scope": "focused" if focused else "full-package", "files": sorted(allowed),
            "total_mutants": total, "killed_mutants": sum(mutation["verified_kill"] for mutation in mutations),
            "reported_killed_mutants": killed, "outcomes": counts,
            "unresolved_mutants": [mutation for mutation in mutations if not mutation["verified_kill"]]}


def check_mutations(summary: dict) -> None:
    unresolved = summary["unresolved_mutants"]
    if unresolved:
        details = ", ".join(f"{entry['path']}:{entry['line']} {entry['operator']} {entry['outcome']}"
                            for entry in unresolved[:10])
        raise MutationError(f"{len(unresolved)} unresolved mutants: {details}")


def positive_timeout(value: float) -> float:
    if not math.isfinite(value) or value <= 0:
        raise MutationError("Process timeout must be a positive, finite number of seconds")
    return value


def terminate_group(process: subprocess.Popen) -> None:
    """Stop compiler and test descendants before removing their working directory."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait()
        return
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def run(arguments: list[str], package: Path, log, *, check: bool = True, timeout: float = 180) -> int:
    positive_timeout(timeout)
    log.write("$ " + " ".join(arguments) + "\n")
    log.flush()
    try:
        process = subprocess.Popen(arguments, cwd=package, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
    except OSError as error:
        raise MutationError(f"Could not run {arguments[0]}: {error}") from None
    try:
        status = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        raise MutationError(f"Command timed out after {timeout:g} seconds: {' '.join(arguments)}") from None
    except KeyboardInterrupt:
        raise MutationError(f"Command interrupted: {' '.join(arguments)}") from None
    finally:
        terminate_group(process)
    if check and status:
        raise MutationError(f"Command failed with exit code {status}: {' '.join(arguments)}")
    return status


def collect(swift: str, muter: str, selected: list[str] | None = None, *, timeout: float = 1800) -> dict:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    report_path = OUTPUT / "report.json"
    summary_path = OUTPUT / "summary.json"
    for artifact in (report_path, summary_path):
        artifact.unlink(missing_ok=True)
    logs_path = OUTPUT / "test-logs"
    if logs_path.exists():
        shutil.rmtree(logs_path)
    positive_timeout(timeout)
    swift, muter = executable(swift), executable(muter)
    expected = production_files(PACKAGE, selected)
    relative_files = [path.relative_to(PACKAGE).as_posix() for path in expected]
    with tempfile.TemporaryDirectory(prefix="tracker-swift-mutations-") as directory:
        package = Path(directory) / "TrackerClient"
        package.mkdir()
        shutil.copy2(PACKAGE / "Package.swift", package / "Package.swift")
        for name in ("Sources", "Tests"):
            shutil.copytree(PACKAGE / name, package / name)
        test_arguments = ["test", "-j", "2", "--parallel", "--num-workers", "4"]
        configuration = {"executable": swift, "arguments": test_arguments,
                         "exclude": ["/Tests/", "/Package.swift"], "mutationTestTimeout": 180}
        configuration_path = package / "muter.conf.yml"
        configuration_path.write_text(json.dumps(configuration, indent=2) + "\n", encoding="utf-8")
        try:
            with (OUTPUT / "run.log").open("w", encoding="utf-8") as log:
                print("Running TrackerClient baseline tests in an isolated package.", flush=True)
                run([swift, *test_arguments], package, log)
                # Swift's module cache records absolute paths. Muter's sibling copy needs a fresh build.
                if (package / ".build").exists():
                    shutil.rmtree(package / ".build")
                command = [muter, "--configuration", str(configuration_path), "--skip-update-check", "--skip-coverage",
                           "--format", "json", "--output", str(report_path)]
                if selected is not None:
                    for filename in relative_files:
                        command.extend(["--files-to-mutate", filename])
                print(f"Running {'focused' if selected is not None else 'full package'} Swift mutations. Log: {OUTPUT / 'run.log'}", flush=True)
                status = run(command, package, log, check=False, timeout=timeout)
        finally:
            preserve_test_logs(package, logs_path)
        try:
            report = json.loads(report_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise MutationError(f"Muter did not produce a valid report: {error}") from None
        summary = mutation_summary(report, package, [package / name for name in relative_files],
                                   focused=selected is not None, logs=logs_path)
        summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
        print(f"Swift mutations ({summary['scope']}): {summary['killed_mutants']}/{summary['total_mutants']} killed")
        print(f"Reports: {OUTPUT}")
        check_mutations(summary)
        if status:
            raise MutationError(f"Muter exited with code {status}. See {OUTPUT / 'run.log'}")
        return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--swift", default="swift", help="Swift executable from the selected toolchain")
    parser.add_argument("--muter", default=str(DEFAULT_MUTER), help="Muter executable, default .build/swift-tools/muter")
    parser.add_argument("--files", nargs="+", help="Package-relative production files for a focused diagnostic run")
    parser.add_argument("--timeout", type=float, default=1800,
                        help="Maximum seconds for the whole Muter run, default 1800")
    arguments = parser.parse_args()
    try:
        collect(arguments.swift, arguments.muter, arguments.files, timeout=arguments.timeout)
    except (MutationError, OSError) as error:
        print(f"Swift mutation testing failed: {error}", file=sys.stderr)
        print(f"Log: {OUTPUT / 'run.log'}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
