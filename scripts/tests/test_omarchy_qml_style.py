import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from contextlib import redirect_stderr


SPEC = importlib.util.spec_from_file_location("omarchy_qml_style", Path(__file__).parents[1] / "omarchy-qml-style.py")
STYLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STYLE)


def messages(text):
    return STYLE.violations(Path("View.qml"), text)


class ViolationTests(unittest.TestCase):
    def test_token_values_pass(self):
        text = "\n".join([
            "spacing: Tokens.spacing.small",
            "anchors.leftMargin: Tokens.spacing.xxLarge + root.labelWidth",
            "font.pixelSize: Tokens.text.caption",
            "radius: Math.min(Tokens.radius.small, theme.radius)",
            "opacity: active ? 1 : Tokens.emphasis.disabled",
            "color: Qt.alpha(theme.selection, Tokens.emphasis.hover)",
        ])
        self.assertEqual(messages(text), [])

    def test_zero_means_none_and_passes(self):
        self.assertEqual(messages("spacing: 0\npadding: 0\nopacity: 0"), [])

    def test_literal_spacing_margins_and_padding_fail(self):
        for line in ("spacing: 8", "Layout.leftMargin: 12", "anchors.margins: -6", "padding: 24", "topMargin: 4"):
            with self.subTest(line=line):
                self.assertEqual(len(messages(line)), 1)

    def test_literal_font_sizes_and_radii_fail(self):
        self.assertEqual(len(messages("font.pixelSize: 13")), 1)
        # A property declaration with a literal default counts too.
        self.assertEqual(len(messages("property int pixelSize: 16")), 1)
        self.assertEqual(len(messages("radius: Math.min(4, theme.radius)")), 1)

    def test_a_literal_inside_an_expression_fails(self):
        self.assertEqual(len(messages("anchors.leftMargin: 32 + root.labelWidth")), 1)
        self.assertEqual(len(messages("opacity: selected ? 0.6 : 1")), 1)

    def test_opacity_may_be_one_but_other_properties_may_not(self):
        self.assertEqual(messages("opacity: running ? 1 : Tokens.emphasis.idleStripe"), [])
        self.assertEqual(len(messages("spacing: 1")), 1)

    def test_literal_tints_fail(self):
        found = messages("color: Qt.alpha(theme.background, 0.7)")
        self.assertEqual(found, ["View.qml:1: Qt.alpha uses 0.7; take it from Tokens.qml"])

    def test_a_divisor_is_a_proportion_and_passes(self):
        self.assertEqual(messages("radius: size / 2"), [])
        self.assertEqual(messages("radius: size/2"), [])
        self.assertEqual(messages("radius: size /  2"), [])
        # Only the divisor itself is a proportion.
        self.assertEqual(len(messages("anchors.leftMargin: width / 2 + 8")), 1)

    def test_grid_spacing_and_side_padding_are_checked(self):
        for line in ("rowSpacing: 18", "columnSpacing: 40", "leftPadding: 6", "horizontalPadding: 10"):
            with self.subTest(line=line):
                self.assertEqual(len(messages(line)), 1)

    def test_every_property_on_a_line_is_checked(self):
        self.assertEqual(len(messages("Text { font.pixelSize: 13 }")), 1)
        self.assertEqual(len(messages("x: 0; spacing: 8")), 1)
        self.assertEqual(len(messages("font { pixelSize: 13 }")), 1)
        found = messages("spacing: 8; padding: 4")
        self.assertEqual(found, ["View.qml:1: spacing uses 8; take it from Tokens.qml",
                                 "View.qml:1: padding uses 4; take it from Tokens.qml"])

    def test_point_sizes_and_readonly_declarations_are_checked(self):
        self.assertEqual(len(messages("font.pointSize: 10")), 1)
        self.assertEqual(len(messages("readonly property int spacing: 8")), 1)

    def test_a_full_tint_passes(self):
        self.assertEqual(messages("color: Qt.alpha(theme.text, 1)"), [])

    def test_comments_are_ignored(self):
        self.assertEqual(messages("spacing: Tokens.spacing.small // was 8"), [])

    def test_messages_name_the_line_and_the_value(self):
        self.assertEqual(messages("x: 1\nspacing: 6"), ["View.qml:2: spacing uses 6; take it from Tokens.qml"])


class CheckTests(unittest.TestCase):
    def test_only_the_shared_tokens_file_is_skipped(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            (source / "Shared").mkdir()
            (source / "Shared" / "Tokens.qml").write_text("readonly property int small: 8\nspacing: 8\n")
            (source / "Features").mkdir()
            (source / "Features" / "Tokens.qml").write_text("spacing: 4\n")
            (source / "View.qml").write_text("spacing: 12\n")
            found = STYLE.check(source)
        self.assertEqual(len(found), 2)
        self.assertTrue(any("Features/Tokens.qml:1" in message for message in found))
        self.assertTrue(any("View.qml:1" in message for message in found))

    def test_main_fails_with_findings_and_passes_without(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            (source / "View.qml").write_text("spacing: Tokens.spacing.small\n")
            self.assertEqual(STYLE.main([str(source)]), 0)
            (source / "Other.qml").write_text("spacing: 6\n")
            errors = io.StringIO()
            with redirect_stderr(errors):
                self.assertEqual(STYLE.main([str(source)]), 1)
        self.assertIn("spacing uses 6", errors.getvalue())

    def test_the_app_follows_the_tokens(self):
        self.assertEqual(STYLE.check(STYLE.SOURCE), [])


if __name__ == "__main__":
    unittest.main()
