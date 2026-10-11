#!/usr/bin/env python3
"""Require pinned Muter and Swift, then run the actual-tool regression tests."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from scripts.swift_muter_tools import run_tool


REGRESSIONS = (
    ("test_swift_muter_ternary", "SwiftMuterTernaryTests",
     "test_optional_comparison_and_nested_branches_compile_and_keep_swap_mutations"),
    ("test_install_swift_muter", "MuterInstallerTests",
     "test_ternary_comparison_mutants_compile_and_are_killed"),
)


def load_module(name: str, path: Path):
    specification = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


def validate_tools() -> tuple[str, str]:
    swift = shutil.which(os.environ.get("SWIFT_EXECUTABLE", "swift"))
    muter = shutil.which(os.environ.get("SWIFT_MUTER_EXECUTABLE", str(ROOT / ".build/swift-tools/muter")))
    if not swift:
        raise ValueError("Swift is required; set SWIFT_EXECUTABLE or add swift to PATH")
    if not muter:
        raise ValueError("Pinned Muter is required; run scripts/install-swift-muter.py")
    installer = load_module("regression_muter_installer", ROOT / "scripts/install-swift-muter.py")
    marker = Path(muter).parent / "muter-revision.txt"
    if not marker.is_file() or marker.read_text(encoding="utf-8").strip() != installer.REVISION:
        raise ValueError("Muter revision does not match the repository pin; run scripts/install-swift-muter.py")
    for executable in (swift, muter):
        result = run_tool([executable, "--version"], timeout=30)
        if result.returncode:
            raise ValueError(f"Cannot run {executable}: {result.stdout}{result.stderr}")
    return swift, muter


def load_regressions() -> unittest.TestSuite:
    suite = unittest.TestSuite()
    for module_name, class_name, test_name in REGRESSIONS:
        module = load_module(f"required_{module_name}", ROOT / f"scripts/tests/{module_name}.py")
        suite.addTest(getattr(module, class_name)(test_name))
    return suite


def main() -> int:
    try:
        swift, muter = validate_tools()
        os.environ["SWIFT_EXECUTABLE"] = swift
        os.environ["SWIFT_MUTER_EXECUTABLE"] = muter
        result = unittest.TextTestRunner(verbosity=2).run(load_regressions())
        if result.skipped or result.testsRun != len(REGRESSIONS):
            print("Required Muter regressions must all run without skips", file=sys.stderr)
            return 1
        return 0 if result.wasSuccessful() else 1
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"Muter regression preflight failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
