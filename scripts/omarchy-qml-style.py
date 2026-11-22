#!/usr/bin/env python3
"""Reject number literals for design-token properties in tt-desktop's QML.

Font sizes, spacing, margins, padding, corner radii, opacities and color tints come from
apps/omarchy/src/Shared/Tokens.qml, so the views share one scale. 0 is allowed, because it
means "none"; 1 is allowed for opacity, because it means "fully shown". A divisor, such
as `size / 2` for a circle, is a proportion and is allowed too.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "apps" / "omarchy" / "src"
TOKENS = Path("Shared") / "Tokens.qml"

# A property whose value must come from a token. It can start a line, follow `{` or `;`, or
# be a declaration with a default; its value runs to the next `;` or `}`.
CHECKED = re.compile(
    r"(?:^|[{;])\s*(?:(?:readonly\s+)?property\s+\w+\s+)?"
    r"(?P<name>(?:font\.)?(?:pixelSize|pointSize)|\w*[Ss]pacing|\w*[Pp]adding|[\w.]*[Mm]argins?|radius|opacity)"
    r"\s*:\s*(?P<value>[^;}]*)"
)
# Qt.alpha(color, 0.5) tints a color, so its factor is an opacity too.
ALPHA = re.compile(r"Qt\.alpha\([^,]+,\s*(?P<value>[0-9.]+)\s*\)")
# A divisor is a proportion, such as half a dot's size for a circle, not a design value.
NUMBER = re.compile(r"(?<![\w.])(?<!/)(\d+(?:\.\d+)?|\.\d+)(?![\w.])")
DIVISOR = re.compile(r"/\s*(\d+(?:\.\d+)?)")


def allowed(name: str, number: str) -> bool:
    value = float(number)
    if value == 0:
        return True
    return name == "opacity" and value == 1


def violations(path: Path, text: str) -> list[str]:
    found: list[str] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        code = line.split("//", 1)[0]
        for match in CHECKED.finditer(code):
            name = match.group("name").split(".")[-1]
            # Divisors are proportions, so they are blanked out before the numbers are read.
            value = DIVISOR.sub("/", match.group("value"))
            for number in NUMBER.findall(value):
                if not allowed(name, number):
                    found.append(f"{path}:{line_number}: {name} uses {number}; take it from Tokens.qml")
        for tint in ALPHA.finditer(code):
            if not allowed("opacity", tint.group("value")):
                found.append(f"{path}:{line_number}: Qt.alpha uses {tint.group('value')}; take it from Tokens.qml")
    return found


def check(source: Path) -> list[str]:
    found: list[str] = []
    for path in sorted(source.rglob("*.qml")):
        if path.relative_to(source) != TOKENS:
            found.extend(violations(path.relative_to(ROOT) if path.is_relative_to(ROOT) else path, path.read_text()))
    return found


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=SOURCE, help="the QML source directory")
    arguments = parser.parse_args(argv)
    found = check(arguments.source)
    for message in found:
        print(message, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
