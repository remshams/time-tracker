"""Check pull request scope selection against committed Git histories."""

from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "swift-mutation-scope.py"
SPEC = importlib.util.spec_from_file_location("swift_mutation_scope", SCRIPT)
scope = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scope)

TRACKING = scope.SOURCES + scope.TRACKING
CATALOG = scope.SOURCES + scope.CATALOG
TRACKING_TEST = scope.TESTS + "Features/Tracking/TrackingStateTests.swift"


class SwiftMutationScopeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="tracker-mutation-scope-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Scope fixture")
        self.git("config", "user.email", "scope@example.test")
        for names in scope.TEST_PRODUCTION.values():
            for name in names:
                self.write(scope.PACKAGE + scope.SOURCES + name, "func example() -> Bool { true }\n")
        self.write(scope.PACKAGE + TRACKING_TEST, "func testTracking() {}\n")
        self.write("README.md", "Fixture documentation\n")
        self.base = self.commit("Initial package")

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

    def select(self, head="HEAD", base=None):
        return scope.select_scope(self.root, base or self.base, head)

    def test_production_changes_from_every_pull_request_commit_select_complete_files(self):
        self.write(scope.PACKAGE + TRACKING, "func tracking() -> Bool { false }\n")
        self.commit("Change tracking")
        self.write(scope.PACKAGE + CATALOG, "func catalog() -> Bool { false }\n")
        head = self.commit("Change catalog")

        result = self.select(head)

        self.assertEqual(result["status"], "focused")
        self.assertEqual(result["files"], sorted([TRACKING, CATALOG]))
        self.assertEqual(result["base"], self.base)
        self.assertEqual(result["head"], head)

    def test_comparison_starts_at_merge_base_even_after_base_branch_advances(self):
        self.git("checkout", "-b", "feature")
        self.write(scope.PACKAGE + TRACKING, "func tracking() -> Bool { false }\n")
        head = self.commit("Change tracking")
        self.git("checkout", "main")
        self.write(scope.PACKAGE + CATALOG, "func baseBranchOnly() {}\n")
        advanced_base = self.commit("Advance main")
        self.git("checkout", "feature")

        result = self.select(head, advanced_base)

        self.assertEqual(result["files"], [TRACKING])
        self.assertEqual(result["merge_base"], self.base)

    def test_test_only_change_mutates_the_exercised_production_file(self):
        self.write(scope.PACKAGE + TRACKING_TEST, "func testTrackingElapsedAfterStopping() {}\n")
        self.commit("Change tracking assertions")

        result = self.select()

        self.assertEqual(result["status"], "focused")
        self.assertEqual(result["files"], [TRACKING])

    def test_integration_suite_selects_feature_coordinator_and_presentation(self):
        test = scope.TESTS + "Features/WorklogMove/WorklogMoveTests.swift"
        self.write(scope.PACKAGE + test, "func testWorklogMoveUpdatesPresentation() {}\n")
        self.commit("Add worklog move assertion")

        result = self.select()

        self.assertEqual(result["status"], "focused")
        self.assertIn(scope.SOURCES + "Features/WorklogMove/WorklogMoveState.swift", result["files"])
        self.assertIn(scope.SOURCES + scope.SESSION, result["files"])
        self.assertIn(scope.SOURCES + scope.OBSERVER, result["files"])
        self.assertIn(scope.SOURCES + scope.TIMESTAMP, result["files"])
        self.assertNotIn(scope.SOURCES + "Features/WindowRouting/TrackerWindowRoutingState.swift", result["files"])

    def test_shared_production_source_still_selects_its_entire_file(self):
        name = scope.SOURCES + scope.MODELS
        self.write(scope.PACKAGE + name, "struct UpdatedModel {}\n")
        self.commit("Change shared model")

        self.assertEqual(self.select()["files"], [name])

    def test_connection_suite_selects_the_state_resets_it_asserts(self):
        test = scope.TESTS + "Features/Connection/ConnectionTests.swift"
        self.write(scope.PACKAGE + test, "func testSourceSwitchResetsHiddenArchiveFailureAndHistory() {}\n")
        self.commit("Change connection state assertions")

        result = self.select()

        self.assertEqual(result["status"], "focused")
        for source in ["Features/TaskArchiving/TaskArchivingState.swift", "Features/TaskCatalog/TaskCatalogState.swift",
                       "Features/WorklogHistory/WorklogHistoryState.swift", "Features/Tracking/TrackingState.swift"]:
            self.assertIn(scope.SOURCES + source, result["files"])
        self.assertNotIn(scope.SOURCES + "Features/WindowRouting/TrackerWindowRoutingState.swift", result["files"])

    def test_rename_suite_selects_the_creation_and_automation_interactions_it_asserts(self):
        test = scope.TESTS + "Features/TaskRename/TaskRenameTests.swift"
        self.write(scope.PACKAGE + test, "func testPauseBeforeRenameAndCreationExclusivity() {}\n")
        self.commit("Change rename interactions")

        result = self.select()

        self.assertEqual(result["status"], "focused")
        self.assertIn(scope.SOURCES + "Features/TaskCreation/TaskCreationState.swift", result["files"])
        self.assertIn(scope.SOURCES + "Features/TrackingAutomation/TrackingAutomationState.swift", result["files"])

    def test_docs_only_change_records_explicit_empty_scope(self):
        self.write("README.md", "Updated documentation\n")
        self.write(scope.PACKAGE + "README.md", "Package documentation\n")
        self.write(scope.PACKAGE + scope.SOURCES + "README.md", "Production layout documentation\n")
        self.write(scope.PACKAGE + scope.TESTS + "README.md", "Test layout documentation\n")
        self.commit("Update documentation")

        result = self.select()

        self.assertEqual(result["status"], "empty")
        self.assertEqual(result["files"], [])
        self.assertTrue(result["reasons"])

    def test_tooling_only_change_records_empty_production_scope(self):
        self.write("scripts/swift-mutations.py", "# Updated mutation tooling\n")
        self.write("scripts/tests/test_swift_muter_ternary.py", "# Updated tool regression\n")
        self.write(".github/workflows/ci.yml", "name: Updated CI\n")
        self.commit("Update CI tooling")

        result = self.select()

        self.assertEqual(result["status"], "empty")
        self.assertEqual(result["files"], [])

    def test_unmapped_test_requires_review_instead_of_a_full_package_run(self):
        self.write(scope.PACKAGE + scope.TESTS + "Features/New/NewTests.swift", "func testNewFeature() {}\n")
        self.commit("Add unmapped suite")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertEqual(result["files"], [])
        self.assertIn("Unmapped test", result["review_reasons"][0])

    def test_shared_test_support_and_package_dependencies_require_review(self):
        for name in [scope.TESTS + "Support/TestSupport.swift", "Package.swift", "Package.resolved"]:
            with self.subTest(path=name):
                self.git("reset", "--hard", self.base)
                self.write(scope.PACKAGE + name, "Updated shared input\n")
                self.commit("Change shared input")
                self.assertEqual(self.select()["status"], "review-required")

    def test_unknown_shared_swift_build_input_requires_review(self):
        self.write(".swiftpm/configuration", "Updated build configuration\n")
        self.commit("Change shared build input")

        self.assertEqual(self.select()["status"], "review-required")

    def test_renamed_source_updates_a_changed_tests_production_mapping(self):
        renamed = scope.SOURCES + "Features/Tracking/ElapsedTrackingState.swift"
        self.git("mv", scope.PACKAGE + TRACKING, scope.PACKAGE + renamed)
        self.write(scope.PACKAGE + TRACKING_TEST, "func testElapsedAfterRename() {}\n")
        self.commit("Rename tracking source and change test")

        result = self.select()

        self.assertEqual(result["status"], "focused")
        self.assertEqual(result["files"], [renamed])
        self.assertEqual(result["changes"][0]["status"], "R100")

    def test_deleted_source_requires_review_and_keeps_other_changed_files(self):
        self.git("rm", scope.PACKAGE + TRACKING)
        self.write(scope.PACKAGE + CATALOG, "func updatedCatalog() {}\n")
        self.commit("Remove tracking source")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertEqual(result["files"], [CATALOG])

    def test_source_moved_out_of_the_package_requires_review(self):
        self.git("mv", scope.PACKAGE + TRACKING, "native-tracking.swift")
        self.commit("Move production source out of portable package")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertEqual(result["files"], [])

    def test_source_moved_into_the_package_selects_the_new_path(self):
        self.write("native-tracking.swift", "struct NativeTracking {}\n")
        self.base = self.commit("Add native tracking source")
        imported = scope.SOURCES + "Features/Tracking/ImportedTracking.swift"
        self.git("mv", "native-tracking.swift", scope.PACKAGE + imported)
        self.commit("Move native source into portable package")

        result = self.select()

        self.assertEqual(result["status"], "focused")
        self.assertEqual(result["files"], [imported])

    def test_deleted_mapped_test_still_selects_the_exercised_source(self):
        self.git("rm", scope.PACKAGE + TRACKING_TEST)
        self.commit("Remove tracking assertion")

        self.assertEqual(self.select()["files"], [TRACKING])

    def test_stale_test_mapping_requires_review(self):
        self.git("rm", scope.PACKAGE + TRACKING)
        self.base = self.commit("Remove production source before feature branch")
        self.write(scope.PACKAGE + TRACKING_TEST, "func testElapsed() {}\n")
        self.commit("Change test with stale mapping")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertIn("stale production source", result["review_reasons"][0])

    def test_source_symlink_cannot_select_outside_the_package(self):
        outside = self.root / "outside.swift"
        outside.write_text("func outsidePackage() {}\n", encoding="utf-8")
        source = self.root / scope.PACKAGE / TRACKING
        source.unlink()
        source.symlink_to(outside)
        self.commit("Replace production source with symlink")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertEqual(result["files"], [])

    def test_uncommitted_symlink_is_rejected_even_when_head_tree_is_regular(self):
        self.write(scope.PACKAGE + TRACKING, "func changedTracking() {}\n")
        self.commit("Change production source")
        source = self.root / scope.PACKAGE / TRACKING
        source.unlink()
        source.symlink_to(self.root / "README.md")

        self.assertEqual(self.select()["status"], "review-required")

    def test_unsafe_new_source_name_never_reaches_muter_arguments(self):
        self.write(scope.PACKAGE + scope.SOURCES + "Features/Tracking/unsafe\nname.swift", "func unsafe() {}\n")
        self.commit("Add source with unsafe filename")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertEqual(result["files"], [])

    def test_git_filenames_cannot_execute_shell_commands(self):
        self.write("docs/$(touch unexpected).md", "Documentation\n")
        self.commit("Add shell characters in documentation filename")

        self.assertEqual(self.select()["status"], "empty")
        self.assertFalse((self.root / "unexpected").exists())

    def test_requested_head_must_match_the_checkout(self):
        self.write(scope.PACKAGE + TRACKING, "func changedTracking() {}\n")
        self.commit("Change production")

        with self.assertRaisesRegex(scope.ScopeError, "Check out the requested head"):
            self.select(self.base)

    def test_revision_cannot_be_interpreted_as_a_git_option(self):
        with self.assertRaises(scope.ScopeError):
            self.select(base="--help")

    def test_cli_writes_review_artifact_and_returns_exit_two(self):
        self.write(scope.PACKAGE + "Package.swift", "Updated dependencies\n")
        script = self.root / "scripts/swift-mutation-scope.py"
        script.parent.mkdir()
        shutil.copy2(SCRIPT, script)
        self.commit("Change dependency inputs")
        artifact = self.root / ".build/scope.json"

        result = subprocess.run([sys.executable, str(script), "--base", self.base, "--head", "HEAD",
                                 "--output", str(artifact)], capture_output=True, text=True)

        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(json.loads(artifact.read_text())["status"], "review-required")


class SwiftMutationScopeMapTests(unittest.TestCase):
    def test_every_current_suite_has_a_mapping_to_existing_production_files(self):
        package = SCRIPT.parents[1] / scope.PACKAGE
        test_root = package / scope.TESTS
        tests = {path.relative_to(test_root).as_posix() for path in test_root.rglob("*.swift")
                 if "Support" not in path.relative_to(test_root).parts}
        self.assertEqual(set(scope.TEST_PRODUCTION), tests)
        for test, sources in scope.TEST_PRODUCTION.items():
            with self.subTest(test=test):
                self.assertTrue(sources)
                self.assertEqual(len(sources), len(set(sources)))
                for source in sources:
                    self.assertTrue((package / scope.SOURCES / source).is_file(), source)


if __name__ == "__main__":
    unittest.main()
