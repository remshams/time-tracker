#!/usr/bin/env python3
"""Build the checked-out revision as the tt-desktop Arch package and install it on this machine."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_FILES = ROOT / "apps/omarchy/pkgbuild"
CARGO_TARGET_DIR = ROOT / ".build/omarchy-cargo"
INSTALLED_BINARY = "/usr/bin/tt-desktop"
PROCESS_NAME = "tt-desktop"
QUIT_TIMEOUT = 10


def output(command: list[str], cwd: Path = ROOT, env: dict[str, str] | None = None) -> str:
    return subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, check=True).stdout.strip()


def revision() -> str:
    commit = output(["git", "rev-parse", "--short=8", "HEAD"])
    return f"{commit}-dirty" if output(["git", "status", "--porcelain"]) else commit


def package_environment(build_revision: str, work: Path) -> dict[str, str]:
    # The PKGBUILD reads the checkout from TT_SOURCE_DIR and keeps the Rust build cache in CARGO_TARGET_DIR, so a
    # second install only rebuilds what changed. PKGDEST puts the package next to the PKGBUILD copy.
    return {**os.environ, "TT_SOURCE_DIR": str(ROOT), "TT_GIT_REVISION": build_revision,
            "CARGO_TARGET_DIR": str(CARGO_TARGET_DIR), "PKGDEST": str(work)}


def build(build_revision: str, work: Path) -> Path:
    # makepkg runs in a temporary copy of the package files, so the checkout gets no pkg/ or src/ folders and stays
    # clean. Its output streams to the terminal because -s can ask for the sudo password to install build tools.
    for source in PACKAGE_FILES.iterdir():
        if source.is_file():
            shutil.copy2(source, work / source.name)
    environment = package_environment(build_revision, work)
    result = subprocess.run(["makepkg", "-fs", "--noconfirm", "--noprogressbar"], cwd=work, env=environment,
                            check=False)
    if result.returncode != 0:
        raise ValueError(f"makepkg failed with exit code {result.returncode}")
    packages = output(["makepkg", "--packagelist"], cwd=work, env=environment).splitlines()
    built = [Path(line) for line in packages if Path(line).is_file()]
    if len(built) != 1:
        raise ValueError(f"Expected one built {PROCESS_NAME} package in {work}, found {len(built)}")
    return built[0]


def is_running() -> bool:
    return subprocess.run(["pgrep", "-x", PROCESS_NAME], capture_output=True, check=False).returncode == 0


def quit_app() -> None:
    # pkill exits 1 when nothing runs, which is fine. SIGTERM lets Qt close the window; tracking lives in the
    # database, so quitting never stops a running timer.
    subprocess.run(["pkill", "-x", PROCESS_NAME], capture_output=True, check=False)
    deadline = time.monotonic() + QUIT_TIMEOUT
    while is_running():
        if time.monotonic() >= deadline:
            raise ValueError(f"Time Tracker did not quit within {QUIT_TIMEOUT} seconds; quit it and try again")
        time.sleep(0.2)


def install(package: Path) -> None:
    # pacman needs root and may prompt for the sudo password, so it gets the terminal.
    result = subprocess.run(["sudo", "pacman", "-U", "--noconfirm", str(package)], check=False)
    if result.returncode != 0:
        raise ValueError(f"pacman failed with exit code {result.returncode}")


def launch() -> None:
    # A new session detaches the app from this script and its terminal, so closing either leaves it running.
    subprocess.Popen([INSTALLED_BINARY], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, start_new_session=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--no-launch", action="store_true", help="Install without starting the app")
    arguments = parser.parse_args()
    try:
        build_revision = revision()
        with tempfile.TemporaryDirectory(prefix="tt-desktop-package-") as directory:
            package = build(build_revision, Path(directory))
            quit_app()
            install(package)
        if not arguments.no_launch:
            launch()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        detail = error.stderr.strip() if isinstance(error, subprocess.CalledProcessError) else ""
        print(f"Install failed: {error}" + (f"\n{detail}" if detail else ""), file=sys.stderr)
        return 1
    print(f"Installed {build_revision} at {INSTALLED_BINARY}" + ("" if arguments.no_launch else " and launched it"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
