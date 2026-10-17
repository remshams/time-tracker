#!/usr/bin/env python3
"""Decide whether a CI run needs the native macOS jobs."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
# Paths that no native build, test, or macOS Rust check reads. Every other path runs the native jobs.
NON_NATIVE_DIRECTORIES = ("docs/", ".githooks/", "scripts/tests/")
NON_NATIVE_FILES = {
    ".cargo-crap.toml",
    ".cargo/mutants.toml",
    ".github/workflows/dependencies.yml",
    ".github/workflows/mutations.yml",
    "scripts/audit-omarchy-themes.py",
    "scripts/deploy-server.py",
}
NO_BASE = {"", "0" * 40}


class ScopeError(Exception):
    pass


def affects_native(path: str) -> bool:
    return not (path.endswith(".md") or path.startswith(NON_NATIVE_DIRECTORIES) or path in NON_NATIVE_FILES)


def changed_paths(root: Path, base: str, head: str, *, merge_base: bool) -> list[str]:
    revisions = [f"{base}...{head}"] if merge_base else [base, head]
    # Without rename detection, a move lists both its old and new path.
    result = subprocess.run(["git", "-C", str(root), "diff", "--no-ext-diff", "--no-renames", "--name-only", "-z",
                             *revisions, "--"], capture_output=True, check=False)
    if result.returncode:
        raise ScopeError(result.stderr.decode("utf-8", errors="replace").strip() or "Git command failed")
    return [path for path in result.stdout.decode("utf-8", errors="surrogateescape").split("\0") if path]


def native_scope(root: Path, event: str, base: str, head: str) -> tuple[bool, str]:
    if event not in ("pull_request", "push"):
        return True, f"{event} runs every check"
    if base in NO_BASE:
        return True, "no previous commit to compare"
    try:
        paths = changed_paths(root, base, head, merge_base=event == "pull_request")
    except ScopeError as error:
        return True, f"could not compare the changes: {error}"
    if not paths:
        return True, "no changed files found"
    native = [path for path in paths if affects_native(path)]
    if native:
        return True, f"{len(native)} of {len(paths)} changed files can affect native checks, including {native[0]}"
    return False, f"none of the {len(paths)} changed files affect native checks"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", required=True, help="GitHub event name")
    parser.add_argument("--base", default="", help="Pull request base or previous pushed commit")
    parser.add_argument("--head", required=True, help="Pull request head or pushed commit")
    parser.add_argument("--github-output", type=Path, required=True, help="File that receives the native output")
    arguments = parser.parse_args()
    native, reason = native_scope(ROOT, arguments.event, arguments.base, arguments.head)
    with arguments.github_output.open("a", encoding="utf-8") as output:
        output.write(f"native={'true' if native else 'false'}\n")
    print(f"Native checks {'required' if native else 'skipped'}: {reason}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
