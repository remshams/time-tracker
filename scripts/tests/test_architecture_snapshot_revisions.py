from pathlib import Path
import re
import subprocess
import unittest


ROOT = Path(__file__).parents[2]
DOCUMENTS = [
    "docs/architecture/README.md",
    "docs/architecture/primitives.md",
    "docs/architecture/primitives.html",
    "docs/architecture/diagrams.html",
]
REVISION = r"([0-9a-f]{7,40})\b"
# Revisions appear after these markers only. CSS colors, quality row IDs and SVG data
# also contain hexadecimal runs, so the check does not scan for bare hex.
MARKERS = [
    re.compile(r"\b(?:revisions?|at|describes?|retains?|scope:)\s+(?:<code>|`)?" + REVISION),
    re.compile(r"·\s+" + REVISION),
    re.compile(r"\bthe\s+" + REVISION + r"\s+baseline\b"),
    re.compile(r"\bbaseline\s+is\s+(?:<code>|`)?" + REVISION),
    re.compile(r"/(?:blob|tree|commit)/" + REVISION),
]


def cited_revisions(text):
    return {match.group(1) for marker in MARKERS for match in marker.finditer(text)}


def git(*arguments):
    return subprocess.run(["git", "-C", str(ROOT), *arguments], capture_output=True, text=True)


class CitedRevisionExtractionTests(unittest.TestCase):
    def test_extracts_revisions_after_snapshot_markers(self):
        text = (
            "Source scope: the core at `e4eb464d5b23fec9dee6c5953a2266e3bf02ed20`, inspected on 16 October 2026. "
            "Report validation scope: `ff6099d`. "
            "<span>Report requests · 16 October 2026 · be493ab</span> "
            "measurements for revision <code>114079d</code>; contracts describe 0db93c1; "
            "the inventory retains revision 6616dd7; Unchanged contracts retain the 1234567 baseline. "
            "The instrumentation baseline is 3be12ca. "
            '<a href="https://github.com/remshams/time-tracker/blob/a0295f8064f54ec748f6fcd632fdadf0e03d1f5e/README.md">'
        )
        self.assertEqual(cited_revisions(text), {
            "e4eb464d5b23fec9dee6c5953a2266e3bf02ed20", "ff6099d", "be493ab", "114079d", "0db93c1", "6616dd7",
            "1234567", "3be12ca", "a0295f8064f54ec748f6fcd632fdadf0e03d1f5e",
        })

    def test_ignores_hexadecimal_text_outside_revision_markers(self):
        text = (
            '.card{box-shadow:0 4px 20px #142a4205} <tr id="quality-df94b01caf98"> '
            "<path d=\"M 1234567 0\"/> at 2026 the shared backend at 12 October"
        )
        self.assertEqual(cited_revisions(text), set())


class ArchitectureSnapshotRevisionTests(unittest.TestCase):
    def setUp(self):
        shallow = git("rev-parse", "--is-shallow-repository")
        if shallow.returncode != 0:
            self.skipTest("The checkout has no Git history.")
        if shallow.stdout.strip() == "true":
            self.skipTest("Shallow clones lack the history needed to check cited revisions.")

    def test_cited_revisions_are_ancestors_of_head(self):
        for document in DOCUMENTS:
            path = ROOT / document
            if not path.exists():
                continue
            for revision in sorted(cited_revisions(path.read_text(encoding="utf-8"))):
                with self.subTest(document=document, revision=revision):
                    result = git("merge-base", "--is-ancestor", f"{revision}^{{commit}}", "HEAD")
                    self.assertEqual(result.returncode, 0,
                                     f"{document} cites {revision}, which is not an ancestor of HEAD.")


if __name__ == "__main__":
    unittest.main()
