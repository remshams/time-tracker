import importlib.util
import io
import json
import shutil
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from unittest import mock


SPEC = importlib.util.spec_from_file_location("crate_dependencies", Path(__file__).parents[1] / "crate-dependencies.py")
CRATES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CRATES)

DOC = """\
<!-- crate-dependencies: core -->
```mermaid
---
title: Rust core crates
---
flowchart TD
    client["tracker-client<br>crates/tracker-client<br>Selects the backend"]:::module
    storage["tracker-storage<br>crates/tracker-storage<br>SQLite"]:::module
    domain["tracker-domain<br>crates/tracker-domain<br>Rules"]:::module
    client --> storage & domain
    storage --> domain
```
<!-- /crate-dependencies: core -->

<!-- crate-dependencies: applications -->
| Application | Rust core crates |
| --- | --- |
| `tracker-tui` | client, domain |
<!-- /crate-dependencies: applications -->
"""

EDGES = {
    "tracker-client": ["tracker-storage", "tracker-domain"],
    "tracker-storage": ["tracker-domain"],
    "tracker-domain": [],
    "tracker-tui": ["tracker-client", "tracker-domain"],
}


def metadata(edges=EDGES, extra=None):
    packages = []
    for name, uses in edges.items():
        dependencies = [{"name": use, "kind": None} for use in uses]
        dependencies += (extra or {}).get(name, [])
        packages.append({"name": name, "dependencies": dependencies})
    return {"packages": packages}


def problems(doc=DOC, meta=None):
    return CRATES.check(doc, meta or metadata())


class CheckTests(unittest.TestCase):
    def test_matching_doc_passes(self):
        self.assertEqual(problems(), [])

    def test_external_dependencies_are_ignored(self):
        extra = {"tracker-domain": [{"name": "serde", "kind": None}]}
        self.assertEqual(problems(meta=metadata(extra=extra)), [])

    def test_dev_and_build_dependencies_are_ignored(self):
        extra = {"tracker-domain": [{"name": "tracker-client", "kind": "dev"}, {"name": "tracker-storage", "kind": "build"}]}
        self.assertEqual(problems(meta=metadata(extra=extra)), [])

    def test_cargo_edge_missing_from_the_core_block(self):
        doc = DOC.replace("client --> storage & domain", "client --> domain")
        self.assertEqual(
            problems(doc),
            ["Cargo has tracker-client -> tracker-storage. Add `storage` to the `client -->` line of the core block."],
        )

    def test_stale_edge_in_the_core_block(self):
        doc = DOC.replace("storage --> domain", "storage --> domain & client")
        result = problems(doc)
        self.assertEqual(len(result), 1)
        self.assertIn("Remove `client` from the `storage -->` line of the core block", result[0])

    def test_edges_with_ampersand_on_the_source_side(self):
        doc = DOC.replace("client --> storage & domain\n    storage --> domain", "client --> storage\n    client & storage --> domain")
        self.assertEqual(problems(doc), [])

    def test_table_row_that_does_not_match(self):
        doc = DOC.replace("| client, domain |", "| client, storage |")
        result = problems(doc)
        self.assertEqual(len(result), 2)
        self.assertIn("Add `domain` to the `tracker-tui` row", result[0])
        self.assertIn("Remove `storage` from the `tracker-tui` row", result[1])

    def test_missing_markers(self):
        doc = DOC.replace("<!-- crate-dependencies: core -->", "").replace("<!-- /crate-dependencies: core -->", "")
        self.assertEqual(len(problems(doc)), 1)
        self.assertIn("Missing `<!-- crate-dependencies: core -->`", problems(doc)[0])
        doc = DOC.replace("<!-- crate-dependencies: applications -->", "")
        self.assertIn("Missing `<!-- crate-dependencies: applications -->`", problems(doc)[0])

    def test_arrow_the_check_cannot_read_fails(self):
        arrows = (
            "storage -->|uses| domain",
            "storage -.-> domain",
            "client --> storage --> domain",
            'storage["tracker-storage"] --> domain["tracker-domain"]',
        )
        for arrow in arrows:
            with self.subTest(arrow=arrow):
                doc = DOC.replace("    storage --> domain\n", f"    storage --> domain\n    {arrow}\n")
                self.assertEqual(
                    problems(doc),
                    [f"The core block has an edge the check can't read: `{arrow}`. Write it as `a --> b & c`."],
                )

    def test_application_without_core_crates(self):
        doc = DOC.replace("| `tracker-tui` | client, domain |", "| `tracker-tui` | client, domain |\n| `tracker-cli` | - |")
        self.assertEqual(problems(doc, metadata({**EDGES, "tracker-cli": []})), [])

    def test_unknown_alias(self):
        doc = DOC.replace("storage --> domain", "storage --> domain & ghost")
        self.assertIn("alias `ghost`", problems(doc)[0])

    def test_package_missing_from_the_doc(self):
        meta = metadata({**EDGES, "tracker-cli": []})
        self.assertEqual(
            problems(meta=meta),
            ["Workspace package `tracker-cli` is not in the doc. Add a node to the core block or a row to the table."],
        )

    def test_crate_that_is_not_a_workspace_package(self):
        doc = DOC.replace("| `tracker-tui` |", "| `tracker-nope` | domain |\n| `tracker-tui` |")
        self.assertTrue(any("`tracker-nope` is in the doc but is not a workspace package" in p for p in problems(doc)))


class MainTests(unittest.TestCase):
    def run_main(self, doc, meta):
        with tempfile.TemporaryDirectory() as folder:
            doc_path = Path(folder) / "architecture.md"
            meta_path = Path(folder) / "metadata.json"
            doc_path.write_text(doc, encoding="utf-8")
            meta_path.write_text(json.dumps(meta), encoding="utf-8")
            stderr = io.StringIO()
            with redirect_stderr(stderr):
                code = CRATES.main(["--doc", str(doc_path), "--metadata", str(meta_path)])
            return code, stderr.getvalue()

    def test_exit_code_is_zero_on_a_match(self):
        self.assertEqual(self.run_main(DOC, metadata()), (0, ""))

    def test_exit_code_is_one_with_messages_on_a_mismatch(self):
        code, output = self.run_main(DOC.replace("client --> storage & domain", "client --> domain"), metadata())
        self.assertEqual(code, 1)
        self.assertIn("Add `storage` to the `client -->` line", output)

    def test_cargo_failure_prints_its_error(self):
        failed = subprocess.CompletedProcess([], 101, stdout="", stderr="error: the lock file needs to be updated\n")
        stderr = io.StringIO()
        with mock.patch.object(CRATES.subprocess, "run", return_value=failed), redirect_stderr(stderr):
            code = CRATES.main([])
        self.assertEqual(code, 1)
        self.assertIn("cargo metadata: error: the lock file needs to be updated", stderr.getvalue())

    @unittest.skipUnless(shutil.which("cargo"), "cargo is not installed")
    def test_real_repository_doc_matches_cargo(self):
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            code = CRATES.main([])
        self.assertEqual((code, stderr.getvalue()), (0, ""))


if __name__ == "__main__":
    unittest.main()
