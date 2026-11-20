#!/usr/bin/env python3
"""Build the checked-out revision in Release and install it as ~/Applications/TimeTracker.app."""

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "apps/swiftui/TimeTracker.xcodeproj"
DERIVED_DATA = ROOT / ".build/native-release"
BUILT_APP = DERIVED_DATA / "Build/Products/Release/TimeTracker.app"
INSTALLED_APP = Path.home() / "Applications/TimeTracker.app"
QUIT_TIMEOUT = 10
BUILD_ERROR_LINES = 20


def output(command: list[str]) -> str:
    return subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()


def revision() -> str:
    commit = output(["git", "rev-parse", "--short=8", "HEAD"])
    return f"{commit}-dirty" if output(["git", "status", "--porcelain"]) else commit


def build(build_revision: str) -> None:
    result = subprocess.run(
        ["xcodebuild", "-project", str(PROJECT), "-scheme", "TimeTracker", "-configuration", "Release",
         "-destination", "generic/platform=macOS", "-derivedDataPath", str(DERIVED_DATA),
         f"TT_GIT_REVISION={build_revision}", "build"],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        lines = (result.stdout + result.stderr).splitlines()
        errors = [line for line in lines if "error:" in line] or lines
        raise ValueError("Release build failed:\n" + "\n".join(errors[-BUILD_ERROR_LINES:]))


def bundle_identifier(app: Path) -> str:
    return output(["plutil", "-extract", "CFBundleIdentifier", "raw", "-o", "-", str(app / "Contents/Info.plist")])


def is_running(identifier: str) -> bool:
    return output(["osascript", "-e", f'application id "{identifier}" is running']) == "true"


def quit_app(identifier: str) -> None:
    # Quits every copy with this bundle ID, including a Debug build, because two copies would both claim the menu bar.
    if not is_running(identifier):
        return
    output(["osascript", "-e", f'tell application id "{identifier}" to quit'])
    deadline = time.monotonic() + QUIT_TIMEOUT
    while is_running(identifier):
        if time.monotonic() > deadline:
            raise ValueError(f"Time Tracker did not quit within {QUIT_TIMEOUT} seconds; quit it and try again")
        time.sleep(0.2)


def install(built: Path, installed: Path) -> None:
    if installed.exists():
        shutil.rmtree(installed)
    installed.parent.mkdir(parents=True, exist_ok=True)
    output(["ditto", str(built), str(installed)])
    output(["codesign", "--verify", "--deep", "--strict", str(installed)])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-launch", action="store_true", help="Install without starting the app")
    arguments = parser.parse_args()
    try:
        build_revision = revision()
        build(build_revision)
        quit_app(bundle_identifier(BUILT_APP))
        install(BUILT_APP, INSTALLED_APP)
        if not arguments.no_launch:
            output(["open", str(INSTALLED_APP)])
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        detail = error.stderr.strip() if isinstance(error, subprocess.CalledProcessError) else ""
        print(f"Install failed: {error}" + (f"\n{detail}" if detail else ""), file=sys.stderr)
        return 1
    print(f"Installed {build_revision} at {INSTALLED_APP}" + ("" if arguments.no_launch else " and launched it"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
