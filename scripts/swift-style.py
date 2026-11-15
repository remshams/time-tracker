#!/usr/bin/env python3
"""Check Swift formatting and lint rules, or apply formatting."""

import argparse
import importlib.util
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("swift_style_installer", ROOT / "scripts/install-swift-style-tools.py")
INSTALLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALLER)
SOURCE_DIRECTORIES = (
    "apps/swiftui/Sources",
    "apps/swiftui/TrackerClient/Sources",
    "apps/swiftui/TrackerClient/Tests",
)
MANIFEST = "apps/swiftui/TrackerClient/Package.swift"
CHECK_INPUTS = {".swift-format", ".swiftlint.yml", "scripts/swift-style.py",
                "scripts/install-swift-style-tools.py", "scripts/swift-style-tools.resolved"}
# Views take padding, stack, grid and spacer spacing, corner radii, fixed font sizes and opacities from the design
# tokens. 0 is allowed everywhere, and 1 in opacity, because hiding or showing a view isn't a design value. The check
# skips string contents and comments.
# SwiftLint's custom rules need SourceKit, which the Linux check doesn't have, so this check runs here.
TOKEN_SCOPE = "apps/swiftui/Sources/TimeTrackerSwiftUI"
TOKEN_FOLDER = f"{TOKEN_SCOPE}/Shared/DesignTokens"
TOKEN_ARGUMENTS = (r"\.(?:padding|cornerRadius)\([^)]*?"
                   r"|\b(?:spacing|horizontalSpacing|verticalSpacing|minLength|cornerRadius):[^,)]*?"
                   r"|\.system\(size:[^,)]*?")
TOKEN_LITERAL = re.compile(rf"(?:{TOKEN_ARGUMENTS})(?<![\w.])(?!0(?:\.0+)?(?![\w.]))\d"
                           r"|\.opacity\([^)]*?(?<![\w.])(?![01](?:\.0+)?(?![\w.]))\d")
SWIFT_STRING = re.compile(r'"(?:[^"\\]|\\.)*"')


def source_files(root: Path) -> list[Path]:
    files = [root / MANIFEST]
    for directory in SOURCE_DIRECTORIES:
        files.extend((root / directory).rglob("*.swift"))
    return sorted(files)


def needs_staged_check() -> bool:
    output = subprocess.check_output(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"], cwd=ROOT,
    ).decode("utf-8")
    return any(path in CHECK_INPUTS or path == MANIFEST or
               (path.endswith(".swift") and any(path.startswith(directory + "/")
                                               for directory in SOURCE_DIRECTORIES))
               for path in output.split("\0"))


def token_literal_violations(root: Path) -> list[str]:
    violations = []
    for path in sorted((root / TOKEN_SCOPE).rglob("*.swift")):
        if path.is_relative_to(root / TOKEN_FOLDER):
            continue
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            code = SWIFT_STRING.sub('""', line).split("//", 1)[0]
            if TOKEN_LITERAL.search(code):
                violations.append(f"{path.relative_to(root)}:{number}: {line.strip()}")
    return violations


def tool(name: str, version: str) -> str:
    binary = INSTALLER.OUTPUT / name
    if not binary.is_file():
        raise ValueError("Install Swift style tools with python3 scripts/install-swift-style-tools.py")
    actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if actual != version:
        raise ValueError(f"Expected {name} {version}, found {actual}; reinstall Swift style tools")
    return str(binary)


def check(*, format_files: bool, format_only: bool) -> None:
    formatter = tool("swift-format", INSTALLER.FORMAT_VERSION)
    files = [str(path) for path in source_files(ROOT)]
    arguments = [formatter, "format", "--in-place"] if format_files else [formatter, "lint", "--strict"]
    subprocess.run([*arguments, "--configuration", str(ROOT / ".swift-format"), *files], cwd=ROOT, check=True)
    if not format_files and not format_only:
        linter = tool("swiftlint", INSTALLER.LINT_VERSION)
        subprocess.run([linter, "lint", "--strict", "--quiet", "--disable-sourcekit",
                        "--no-cache", "--config", str(ROOT / ".swiftlint.yml"), *files], cwd=ROOT, check=True)
        if violations := token_literal_violations(ROOT):
            raise ValueError("use a token from Shared/DesignTokens instead of a number literal:\n" +
                             "\n".join(violations))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--format", action="store_true", help="Apply formatting without running SwiftLint")
    modes.add_argument("--format-check", action="store_true", help="Only check formatting")
    parser.add_argument("--staged", action="store_true", help="Skip checks unless staged paths affect Swift style")
    arguments = parser.parse_args()
    if arguments.staged and arguments.format:
        parser.error("--staged cannot apply formatting")
    try:
        if not arguments.staged or needs_staged_check():
            check(format_files=arguments.format, format_only=arguments.format_check)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Swift style check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
