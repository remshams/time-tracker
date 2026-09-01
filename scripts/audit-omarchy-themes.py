#!/usr/bin/env python3
"""Audit the ANSI colors used by the TUI against Omarchy themes.

The role mappings mirror apps/tui/src/styles.rs. Change both files together.
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path


EXPECTED_MODE_COUNTS = {"dark": 17, "light": 5}
ROLES = (
    ("normal foreground/background", "foreground", "background", 4.5),
    ("selected foreground/background", "background", "foreground", 4.5),
    ("title blue/background", "blue", "background", 4.5),
    ("focused border blue/background", "blue", "background", 3.0),
    ("active marker green/background", "green", "background", 3.0),
    ("error label red/background", "red", "background", 3.0),
    ("input cursor foreground/background", "foreground", "background", 4.5),
)
COLOR_NAMES = tuple(
    dict.fromkeys(
        color for _, foreground, background, _ in ROLES for color in (foreground, background)
    )
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("themes_dir", type=Path, help="Omarchy themes directory")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="return failure when a contrast threshold is missed",
    )
    return parser.parse_args()


def escape_terminal(text: str) -> str:
    """Make control characters visible instead of writing them to the terminal."""
    escaped = []
    for character in text:
        codepoint = ord(character)
        if character == "\\":
            escaped.append("\\\\")
        elif character.isprintable():
            escaped.append(character)
        elif codepoint <= 0xFF:
            escaped.append(f"\\x{codepoint:02x}")
        elif codepoint <= 0xFFFF:
            escaped.append(f"\\u{codepoint:04x}")
        else:
            escaped.append(f"\\U{codepoint:08x}")
    return "".join(escaped)


def parse_color(value: object, theme: Path, name: str) -> tuple[int, int, int]:
    if not isinstance(value, str) or len(value) != 7 or value[0] != "#":
        raise ValueError(f"{theme}: {name} must be a #RRGGBB color")
    try:
        return tuple(int(value[index : index + 2], 16) for index in (1, 3, 5))  # type: ignore[return-value]
    except ValueError as error:
        raise ValueError(f"{theme}: {name} must be a #RRGGBB color") from error


def relative_luminance(color: tuple[int, int, int]) -> float:
    channels = []
    for channel in color:
        value = channel / 255
        channels.append(value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4)
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]


def contrast(first: tuple[int, int, int], second: tuple[int, int, int]) -> float:
    lighter = max(relative_luminance(first), relative_luminance(second))
    darker = min(relative_luminance(first), relative_luminance(second))
    return (lighter + 0.05) / (darker + 0.05)


def main() -> int:
    args = parse_args()
    try:
        themes_dir = args.themes_dir
        if not themes_dir.is_dir():
            raise ValueError(f"not a directory: {themes_dir}")
        files = sorted(
            (
                child / "colors.toml"
                for child in themes_dir.iterdir()
                if child.is_dir() and (child / "colors.toml").is_file()
            ),
            key=lambda path: path.parent.name,
        )
        if not files:
            raise ValueError(f"no themes found in {themes_dir}")
        themes = []
        modes = []
        for colors_file in files:
            try:
                with colors_file.open("rb") as stream:
                    data = tomllib.load(stream)
            except (OSError, tomllib.TOMLDecodeError) as error:
                raise ValueError(f"{colors_file}: malformed colors.toml ({error})") from error
            mode = data.get("mode")
            if mode not in EXPECTED_MODE_COUNTS:
                raise ValueError(f"{colors_file}: mode must be dark or light")
            try:
                colors = {
                    name: parse_color(data[name], colors_file, name)
                    for name in COLOR_NAMES
                }
            except KeyError as error:
                raise ValueError(f"{colors_file}: missing color {error.args[0]}") from error
            themes.append((colors_file.parent.name, colors))
            modes.append(mode)

        mode_counts = {mode: modes.count(mode) for mode in EXPECTED_MODE_COUNTS}
        if mode_counts != EXPECTED_MODE_COUNTS:
            raise ValueError(
                "theme inventory must contain exactly 17 dark and 5 light themes; "
                f"found {mode_counts['dark']} dark and {mode_counts['light']} light"
            )
    except (OSError, ValueError, KeyError) as error:
        print(f"error: {escape_terminal(str(error))}", file=sys.stderr)
        return 2

    reports = []
    failure_count = 0
    for role, foreground, background, threshold in ROLES:
        results = [
            (theme, contrast(colors[foreground], colors[background]))
            for theme, colors in themes
        ]
        worst = min(results, key=lambda result: (result[1], result[0]))
        failures = [result for result in results if result[1] < threshold]
        reports.append((role, threshold, worst, failures))
        failure_count += len(failures)

    print(f"Themes: {len(themes)}")
    print(f"Dark: {mode_counts['dark']}")
    print(f"Light: {mode_counts['light']}")
    print(f"Failures: {failure_count}")
    for role, threshold, worst, failures in reports:
        print(f"Role: {role}")
        print(
            f"  Worst: {escape_terminal(worst[0])} | {worst[1]:.2f}:1 "
            f"(threshold {threshold:.1f}:1)"
        )
        print(f"  Failures: {len(failures)}")
        for theme, ratio in failures:
            print(f"    {escape_terminal(theme)} | {ratio:.2f}:1")
    return 1 if args.strict and failure_count else 0


if __name__ == "__main__":
    raise SystemExit(main())
