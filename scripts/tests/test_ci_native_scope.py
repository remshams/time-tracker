"""Check which committed changes require the native macOS CI jobs."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "ci-native-scope.py"
SPEC = importlib.util.spec_from_file_location("ci_native_scope", SCRIPT)
scope = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scope)


class NativeScopeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="tracker-native-scope-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Scope fixture")
        self.git("config", "user.email", "scope@example.test")
        self.write("README.md", "Fixture documentation\n")
        self.write("docs/guide.html", "<p>Guide</p>\n")
        self.write("apps/swiftui/App.swift", "struct App {}\n")
        self.base = self.commit("Initial project")

    def git(self, *arguments):
        result = subprocess.run(["git", "-C", str(self.root), *arguments], capture_output=True, check=True)
        return result.stdout.decode().strip()

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self, message):
        self.git("add", "--all")
        self.git("commit", "-m", message)
        return self.git("rev-parse", "HEAD")

    def scope(self, event="pull_request", base=None):
        return scope.native_scope(self.root, event, self.base if base is None else base, "HEAD")

    def test_documentation_only_changes_skip_native_checks(self):
        self.write("README.md", "Updated documentation\n")
        self.write("apps/swiftui/README.md", "Native notes\n")
        self.write("docs/guide.html", "<p>Updated guide</p>\n")
        self.write("scripts/tests/test_example.py", "pass\n")
        self.write("scripts/deploy-server.py", "pass\n")
        self.write(".github/workflows/mutations.yml", "name: Mutation tests\n")
        self.write(".github/actions/setup-muter/action.yml", "name: Set up Muter\n")
        self.write(".coderabbit.yaml", "reviews: {}\n")
        self.commit("Update documentation and Linux-only tooling")
        for event in ("pull_request", "push"):
            native, reason = self.scope(event)
            self.assertFalse(native, reason)
            self.assertEqual(reason, "none of the 8 changed files affect native checks")

    def test_any_other_change_requires_native_checks(self):
        for name in ("apps/swiftui/App.swift", "crates/tracker-domain/src/lib.rs", "Cargo.lock",
                     "apps/server/src/main.rs", "apps/server/Cargo.toml", ".github/workflows/ci.yml",
                     ".github/actions/setup-rust/action.yml",
                     "scripts/run-native-ui.py", "scripts/ci-native-scope.py", ".cargo/config.toml"):
            with self.subTest(name=name):
                self.write("README.md", f"Changed with {name}\n")
                self.write(name, f"// {name}\n")
                self.commit(f"Change {name}")
                native, reason = self.scope()
                self.assertTrue(native, reason)
                self.assertIn(name, reason)
                self.base = self.git("rev-parse", "HEAD")

    def test_moving_a_native_file_into_documentation_requires_native_checks(self):
        self.git("mv", "apps/swiftui/App.swift", "docs/App.swift")
        self.commit("Move source into documentation")
        native, reason = self.scope()
        self.assertTrue(native, reason)
        self.assertEqual(reason, "1 of 2 changed files can affect native checks, including apps/swiftui/App.swift")

    def test_pull_requests_ignore_changes_that_only_reached_the_base_branch(self):
        self.git("switch", "--create", "feature")
        self.write("docs/guide.html", "<p>Feature guide</p>\n")
        self.commit("Document the feature")
        self.git("switch", "main")
        self.write("apps/swiftui/App.swift", "struct App { let changed = true }\n")
        main = self.commit("Change the app on main")
        self.git("switch", "feature")
        self.assertFalse(self.scope("pull_request", main)[0])
        self.assertTrue(self.scope("push", main)[0])

    def test_runs_without_a_comparable_base_require_native_checks(self):
        self.write("README.md", "Updated documentation\n")
        self.commit("Update documentation")
        self.assertEqual(self.scope("workflow_dispatch"), (True, "workflow_dispatch runs every check"))
        self.assertEqual(self.scope("push", ""), (True, "no previous commit to compare"))
        self.assertEqual(self.scope("push", "0" * 40), (True, "no previous commit to compare"))
        native, reason = self.scope("push", "f" * 40)
        self.assertTrue(native)
        self.assertTrue(reason.startswith("could not compare the changes: "), reason)
        self.assertEqual(self.scope("push", "HEAD"), (True, "no changed files found"))

    def test_command_appends_the_github_output(self):
        self.write("README.md", "Updated documentation\n")
        self.commit("Update documentation")
        # The uncommitted copy resolves the fixture as its repository without changing the compared commits.
        self.write("scripts/ci-native-scope.py", SCRIPT.read_text())
        output = Path(self.directory.name + "-output")
        self.addCleanup(output.unlink, missing_ok=True)
        output.write_text("previous=value\n")
        messages = []
        for event in ("push", "workflow_dispatch"):
            result = subprocess.run([sys.executable, str(self.root / "scripts/ci-native-scope.py"),
                                     "--event", event, "--base", self.base, "--head", "HEAD",
                                     "--github-output", str(output)],
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            messages.append(result.stdout)
        self.assertEqual(output.read_text(), "previous=value\nnative=false\nnative=true\n")
        self.assertEqual(messages, ["Native checks skipped: none of the 1 changed files affect native checks\n",
                                    "Native checks required: workflow_dispatch runs every check\n"])


if __name__ == "__main__":
    unittest.main()
