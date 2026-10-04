#!/usr/bin/env python3
"""Build the pinned Muter tool with Swift 6.1 or newer."""

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
REVISION = "7f1f2584e0a27fc05c952a5c8cdd52b10cc9513f"
SHA256 = "d2d951d66b19fa307c52d59b9976fa0bf0be8cf01d0417fb9b58461a722ec981"
URL = f"https://codeload.github.com/muter-mutation-testing/muter/tar.gz/{REVISION}"
OUTPUT = ROOT / ".build/swift-tools"


def unpack(archive: Path, destination: Path) -> None:
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise ValueError("Muter source archive checksum does not match the pinned revision")
    with tarfile.open(archive) as source:
        source.extractall(destination, filter="data")


def patch_source(source: Path) -> None:
    # Linux has no Objective-C autorelease pools. Upstream's discovery step
    # calls this function unconditionally at the pinned revision.
    (source / "Sources/muterCore/LinuxAutoreleasepool.swift").write_text(
        "#if os(Linux)\n"
        "func autoreleasepool<Result>(invoking body: () throws -> Result) rethrows -> Result {\n"
        "    try body()\n"
        "}\n"
        "#endif\n",
        encoding="utf-8",
    )
    # Upstream reparses files, which gives syntax nodes new identities. Match
    # the original code block by position and text when identity lookup fails.
    mapping = source / "Sources/muterCore/MutationSchemata/SchemataMutationMapping.swift"
    original = "        mappings[codeBlockSyntax]\n"
    replacement = (
        "        mappings[codeBlockSyntax] ?? mappings.first {\n"
        "            $0.key.position == codeBlockSyntax.position &&\n"
        "                $0.key.description == codeBlockSyntax.description\n"
        "        }?.value\n"
    )
    contents = mapping.read_text(encoding="utf-8")
    if contents.count(original) != 1:
        raise ValueError("Pinned Muter source no longer matches the syntax mapping patch")
    mapping.write_text(contents.replace(original, replacement), encoding="utf-8")
    rewriter = source / "Sources/muterCore/Rewriters/MuterRewriter.swift"
    contents = rewriter.read_text(encoding="utf-8")
    replacements = {
        "        guard let mutationSchemata = schemataMappings.schemata(node) else {\n"
        "            return super.visit(node)\n":
        "        let mutationSchemata = schemataMappings.schemata(node)\n"
        "        let rewritten = super.visit(node)\n"
        "        guard let mutationSchemata else {\n"
        "            return rewritten\n",
        "            with: node\n": "            with: rewritten\n",
        "        return super.visit(newNode)\n": "        return newNode\n",
    }
    for original, replacement in replacements.items():
        if contents.count(original) != 1:
            raise ValueError("Pinned Muter source no longer matches the nested syntax patch")
        contents = contents.replace(original, replacement)
    rewriter.write_text(contents, encoding="utf-8")
    contents = mapping.read_text(encoding="utf-8")
    original = "map.fileName"
    if contents.count(original) != 3:
        raise ValueError("Pinned Muter source no longer matches the file path grouping patch")
    mapping.write_text(contents.replace(original, "map.filePath"), encoding="utf-8")


def install(swift: str, jobs: int) -> Path:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="muter-install-") as temporary:
        directory = Path(temporary)
        archive = directory / "source.tar.gz"
        print(f"Downloading Muter {REVISION}", flush=True)
        with urllib.request.urlopen(URL, timeout=60) as response:
            with archive.open("wb") as target:
                shutil.copyfileobj(response, target)
        unpack(archive, directory)
        source = directory / f"muter-{REVISION}"
        patch_source(source)
        build = OUTPUT / "muter-build"
        arguments = [swift, "build", "--package-path", str(source),
                     "--scratch-path", str(build), "--force-resolved-versions",
                     "--product", "muter", "-j", str(jobs)]
        subprocess.run(arguments, check=True)
        binary_directory = subprocess.check_output(
            [swift, "build", "--package-path", str(source), "--scratch-path", str(build),
             "--show-bin-path"], text=True,
        ).strip()
        binary = OUTPUT / "muter"
        staged = OUTPUT / "muter.new"
        shutil.copy2(Path(binary_directory) / "muter", staged)
        os.replace(staged, binary)
        (OUTPUT / "muter-revision.txt").write_text(REVISION + "\n", encoding="utf-8")
        print(f"Installed {binary}")
        return binary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--swift", default="swift")
    parser.add_argument("--jobs", type=int, default=4)
    arguments = parser.parse_args()
    if arguments.jobs < 1:
        parser.error("--jobs must be positive")
    try:
        install(arguments.swift, arguments.jobs)
    except (OSError, ValueError, tarfile.TarError, subprocess.CalledProcessError) as error:
        print(f"Muter installation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
