from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "audit-omarchy-themes.py"
COLORS = {
    "background": "#000000",
    "foreground": "#ffffff",
    "blue": "#ffffff",
    "green": "#ffffff",
    "red": "#ffffff",
}
ROLES = (
    ("normal foreground/background", 4.5),
    ("selected foreground/background", 4.5),
    ("title blue/background", 4.5),
    ("focused border blue/background", 3.0),
    ("active marker green/background", 3.0),
    ("error label red/background", 3.0),
    ("input cursor foreground/background", 4.5),
)


class AuditOmarchyThemesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def write_theme(
        self,
        themes_dir: Path,
        name: str,
        mode: str,
        colors: dict[str, str] | None = None,
    ) -> Path:
        theme_dir = themes_dir / name
        theme_dir.mkdir(parents=True, exist_ok=True)
        values = COLORS if colors is None else colors
        lines = [f"mode = {json.dumps(mode)}"]
        lines.extend(f"{key} = {json.dumps(value)}" for key, value in values.items())
        colors_file = theme_dir / "colors.toml"
        colors_file.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return colors_file

    def make_inventory(
        self,
        themes_dir: Path | None = None,
        dark: int = 17,
        light: int = 5,
    ) -> Path:
        target = self.root / "themes" if themes_dir is None else themes_dir
        for index in range(dark):
            self.write_theme(target, f"dark-{index:02d}", "dark")
        for index in range(light):
            self.write_theme(target, f"light-{index:02d}", "light")
        return target

    def run_audit(self, themes_dir: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *arguments, str(themes_dir)],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_default_reports_misses_and_exits_zero(self) -> None:
        themes_dir = self.make_inventory()
        weak_colors = {**COLORS, "red": "#111111"}
        self.write_theme(themes_dir, "dark-00", "dark", weak_colors)

        result = self.run_audit(themes_dir)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Failures: 1\n", result.stdout)
        self.assertIn("Role: error label red/background", result.stdout)
        self.assertIn("    dark-00 | 1.11:1", result.stdout)

    def test_strict_exits_one_when_a_threshold_is_missed(self) -> None:
        themes_dir = self.make_inventory()
        weak_colors = {**COLORS, "red": "#111111"}
        self.write_theme(themes_dir, "dark-00", "dark", weak_colors)

        result = self.run_audit(themes_dir, "--strict")

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Failures: 1\n", result.stdout)

    def test_invalid_and_missing_directories_exit_two(self) -> None:
        file_path = self.root / "not-a-directory"
        file_path.write_text("content", encoding="utf-8")
        for path in (file_path, self.root / "missing"):
            with self.subTest(path=path):
                result = self.run_audit(path)
                self.assertEqual(result.returncode, 2)
                self.assertIn("error: not a directory:", result.stderr)

    def test_malformed_toml_exits_two(self) -> None:
        themes_dir = self.make_inventory()
        (themes_dir / "dark-00" / "colors.toml").write_text(
            'mode = "dark"\nbackground = [', encoding="utf-8"
        )

        result = self.run_audit(themes_dir)

        self.assertEqual(result.returncode, 2)
        self.assertIn("malformed colors.toml", result.stderr)

    def test_malformed_color_exits_two(self) -> None:
        themes_dir = self.make_inventory()
        colors = {**COLORS, "blue": "not-a-color"}
        self.write_theme(themes_dir, "dark-00", "dark", colors)

        result = self.run_audit(themes_dir)

        self.assertEqual(result.returncode, 2)
        self.assertIn("blue must be a #RRGGBB color", result.stderr)

    def test_missing_color_exits_two(self) -> None:
        themes_dir = self.make_inventory()
        colors = {key: value for key, value in COLORS.items() if key != "green"}
        self.write_theme(themes_dir, "dark-00", "dark", colors)

        result = self.run_audit(themes_dir)

        self.assertEqual(result.returncode, 2)
        self.assertIn("missing color green", result.stderr)

    def test_invalid_mode_exits_two(self) -> None:
        themes_dir = self.make_inventory()
        self.write_theme(themes_dir, "dark-00", "sepia")

        result = self.run_audit(themes_dir)

        self.assertEqual(result.returncode, 2)
        self.assertIn("mode must be dark or light", result.stderr)

    def test_wrong_theme_counts_exit_two(self) -> None:
        cases = ((16, 5), (16, 6), (18, 4))
        for dark, light in cases:
            with self.subTest(dark=dark, light=light):
                themes_dir = self.root / f"themes-{dark}-{light}"
                self.make_inventory(themes_dir, dark=dark, light=light)
                result = self.run_audit(themes_dir)
                self.assertEqual(result.returncode, 2)
                self.assertIn(
                    f"found {dark} dark and {light} light", result.stderr
                )

    def test_role_output_is_deterministic(self) -> None:
        themes_dir = self.make_inventory()
        expected_lines = ["Themes: 22", "Dark: 17", "Light: 5", "Failures: 0"]
        for role, threshold in ROLES:
            expected_lines.extend(
                (
                    f"Role: {role}",
                    f"  Worst: dark-00 | 21.00:1 (threshold {threshold:.1f}:1)",
                    "  Failures: 0",
                )
            )

        first = self.run_audit(themes_dir)
        second = self.run_audit(themes_dir)

        expected = "\n".join(expected_lines) + "\n"
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(first.stdout, expected)
        self.assertEqual(second.stdout, expected)

    def test_theme_names_escape_terminal_control_characters(self) -> None:
        themes_dir = self.make_inventory()
        unsafe_name = "evil\x1b]0;owned\x07\nname"
        (themes_dir / "dark-00").rename(themes_dir / unsafe_name)
        weak_colors = {**COLORS, "red": "#111111"}
        self.write_theme(themes_dir, unsafe_name, "dark", weak_colors)

        result = self.run_audit(themes_dir)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("\x1b", result.stdout)
        self.assertNotIn("\x07", result.stdout)
        escaped_name = r"evil\x1b]0;owned\x07\x0aname"
        self.assertIn(escaped_name, result.stdout)

        (themes_dir / unsafe_name / "colors.toml").write_text("[", encoding="utf-8")
        error_result = self.run_audit(themes_dir)

        self.assertEqual(error_result.returncode, 2)
        self.assertNotIn("\x1b", error_result.stderr)
        self.assertNotIn("\x07", error_result.stderr)
        self.assertIn(escaped_name, error_result.stderr)


if __name__ == "__main__":
    unittest.main()
