#!/usr/bin/env python3
"""Install pinned Swift formatting and linting tools inside the repository."""

import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / ".build/swift-style"
FORMAT_VERSION = "604.0.0"
FORMAT_SHA256 = "43f7f4d039dd85733fdb6119dcd29271d08936592ee9904840f0fd743b061894"
LINT_VERSION = "0.65.1"
LINT_ASSETS = {
    ("Linux", "x86_64"): ("swiftlint_linux_amd64.zip", "swiftlint-static",
                          "caeed6f4a679c35539ffaf124f6c4ab4a8416917f7d8796279dc52b74026059d"),
    ("Linux", "aarch64"): ("swiftlint_linux_arm64.zip", "swiftlint-static",
                           "9ffa52f478e6d8eb485d37d14715ffac90abc81c58f3370d598bf75be05605f8"),
    ("Darwin", "arm64"): ("portable_swiftlint.zip", "swiftlint",
                          "c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0"),
    ("Darwin", "x86_64"): ("portable_swiftlint.zip", "swiftlint",
                           "c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0"),
}
# Python 3.12 added extraction filters, and some patch releases of older versions backported them.
SUPPORTS_TAR_FILTER = hasattr(tarfile, "data_filter")


def download(url: str, destination: Path, checksum: str) -> None:
    with urllib.request.urlopen(url, timeout=60) as response, destination.open("wb") as target:
        shutil.copyfileobj(response, target)
    if hashlib.sha256(destination.read_bytes()).hexdigest() != checksum:
        raise ValueError(f"Checksum mismatch for {destination.name}")


def check_tar_member(member: tarfile.TarInfo, root: str) -> None:
    def inside(path: str) -> bool:
        return os.path.commonpath([root, os.path.realpath(path)]) == root

    name = Path(member.name)
    # Extraction strips a trailing slash, so a link named "link/" is created as "link".
    target = os.path.normpath(os.path.join(root, member.name))
    if name.is_absolute() or ".." in name.parts or not inside(target):
        raise ValueError(f"Archive member {member.name} leaves the target directory")
    if member.issym() and not inside(os.path.join(os.path.dirname(target), member.linkname)):
        raise ValueError(f"Archive link {member.name} points outside the target directory")
    if member.islnk():
        source = os.path.join(root, member.linkname)
        if not inside(source):
            raise ValueError(f"Archive link {member.name} points outside the target directory")
        # Without an existing file, tarfile extracts the linked member again at this path,
        # and a relative symlink there can point outside.
        if not os.path.isfile(source):
            raise ValueError(f"Archive link {member.name} needs an extracted regular file as its target")
    if not (member.isfile() or member.isdir() or member.issym() or member.islnk()):
        raise ValueError(f"Archive member {member.name} is not a file, directory or link")


def extract_tar(archive: Path, directory: Path) -> None:
    with tarfile.open(archive) as source:
        if SUPPORTS_TAR_FILTER:
            source.extractall(directory, filter="data")
            return
        root = os.path.realpath(directory)
        # Extract one member at a time so the check resolves links extracted before it.
        for member in source:
            check_tar_member(member, root)
            source.extract(member, root)


def extract_zip_member(archive: Path, member: str, destination: Path) -> None:
    # Copies one named member to a fixed path, so member names in the archive never become paths.
    with zipfile.ZipFile(archive) as source, destination.open("wb") as target:
        with source.open(member) as contents:
            shutil.copyfileobj(contents, target)


def install_binary(source: Path, name: str, version: str) -> None:
    actual = subprocess.check_output([str(source), "--version"], text=True).strip()
    if actual != version:
        raise ValueError(f"Expected {name} {version}, found {actual}")
    staged = OUTPUT / f"{name}.new"
    shutil.copy2(source, staged)
    staged.chmod(0o755)
    os.replace(staged, OUTPUT / name)


def install(swift: str, jobs: int) -> None:
    asset = LINT_ASSETS.get((platform.system(), platform.machine()))
    if asset is None:
        raise ValueError("Swift style tools support Linux and macOS on x86_64 and arm64")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="swift-style-install-") as temporary:
        directory = Path(temporary)
        archive = directory / "swift-format.tar.gz"
        print(f"Building swift-format {FORMAT_VERSION}", flush=True)
        download(f"https://codeload.github.com/swiftlang/swift-format/tar.gz/refs/tags/{FORMAT_VERSION}",
                 archive, FORMAT_SHA256)
        extract_tar(archive, directory)
        package = directory / f"swift-format-{FORMAT_VERSION}"
        shutil.copy2(ROOT / "scripts/swift-style-tools.resolved", package / "Package.resolved")
        arguments = [swift, "build", "--package-path", str(package),
                     "--scratch-path", str(OUTPUT / "formatter-build"),
                     "--cache-path", str(OUTPUT / "cache"),
                     "--config-path", str(OUTPUT / "config"),
                     "--security-path", str(OUTPUT / "security"),
                     "--configuration", "release", "--force-resolved-versions"]
        subprocess.run([*arguments, "--product", "swift-format", "-j", str(jobs)], check=True)
        binaries = Path(subprocess.check_output([*arguments, "--show-bin-path"], text=True).strip())
        install_binary(binaries / "swift-format", "swift-format", FORMAT_VERSION)
        name, member, checksum = asset
        print(f"Downloading SwiftLint {LINT_VERSION}", flush=True)
        archive = directory / name
        download(f"https://github.com/realm/SwiftLint/releases/download/{LINT_VERSION}/{name}",
                 archive, checksum)
        binary = directory / "swiftlint"
        extract_zip_member(archive, member, binary)
        binary.chmod(0o755)
        install_binary(binary, "swiftlint", LINT_VERSION)
    print(f"Installed style tools in {OUTPUT}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--swift", default="swift")
    parser.add_argument("--jobs", type=int, default=4)
    arguments = parser.parse_args()
    if arguments.jobs < 1:
        parser.error("--jobs must be positive")
    try:
        install(arguments.swift, arguments.jobs)
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        print(f"Swift style tool installation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
