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
RENAME_TEST = scope.TESTS + "Features/TaskRename/TaskRenameTests.swift"
ARCHIVING_TEST = scope.TESTS + "Features/TaskArchiving/TaskArchivingTests.swift"
SUPPORT = scope.TESTS + "Support/TestSupport.swift"
CONTRACT = scope.SOURCES + scope.DEPENDENCIES
DELETION_TEST = scope.TESTS + "Features/WorklogDeletion/WorklogDeletionTests.swift"

SUPPORT_BEFORE = """import Foundation
@MainActor
final class FakeClient: TrackerClient {
    enum Operation: Equatable {
        case archive(task: String, at: String)
        case unarchive(task: String, at: String)
        case rename(task: String, name: String, at: String)
    }
    enum Reply { case task(TaskCommandResult) }
    struct Request {
        let operation: Operation
        let complete: (Result<Reply, Error>) -> Void
        func succeed(_ resources: TaskListResources) {
            switch operation {
            case .rename(let id, _, _), .archive(let id, _), .unarchive(let id, _):
                let task = resources.catalog.value.first { $0.id == id }!
                let desiredName: String?
                let desiredArchived: Bool?
                switch operation {
                case .rename(_, let name, _):
                    desiredName = TaskNameEditingPolicy.normalized(name); desiredArchived = nil
                case .archive: desiredName = nil; desiredArchived = true
                case .unarchive: desiredName = nil; desiredArchived = false
                }
                complete(.success(.task(TaskCommandResult(task: task, receipt: nil))))
            }
        }
    }
    var requests: [Request] = []
    func shutdown() { requests.removeAll() }
    func archiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult {
        try await taskReply(.archive(task: taskID, at: occurredAt))
    }
    func renameTask(taskID: String, name: String, occurredAt: String) async throws -> TaskCommandResult {
        try await taskReply(.rename(task: taskID, name: name, at: occurredAt))
    }
}
"""
SUPPORT_AFTER = """import Foundation
@MainActor
final class FakeClient: TrackerClient {
    enum Operation: Equatable {
        case archive(task: String, expectedName: String, at: String)
        case unarchive(task: String, at: String)
        case rename(task: String, expectedName: String, name: String, at: String)
    }
    enum Reply { case task(TaskCommandResult) }
    struct Request {
        let operation: Operation
        let complete: (Result<Reply, Error>) -> Void
        func succeed(_ resources: TaskListResources) {
            switch operation {
            case .rename(let id, _, _, _), .archive(let id, _, _), .unarchive(let id, _):
                let task = resources.catalog.value.first { $0.id == id }!
                let desiredName: String?
                let desiredArchived: Bool?
                switch operation {
                case .rename(_, _, let name, _):
                    desiredName = TaskNameEditingPolicy.normalized(name); desiredArchived = nil
                case .archive: desiredName = nil; desiredArchived = true
                case .unarchive: desiredName = nil; desiredArchived = false
                }
                complete(.success(.task(TaskCommandResult(task: task, receipt: nil))))
            }
        }
    }
    var requests: [Request] = []
    func shutdown() { requests.removeAll() }
    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult {
        try await taskReply(.archive(task: taskID, expectedName: expectedName, at: occurredAt))
    }
    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws
        -> TaskCommandResult
    {
        try await taskReply(.rename(task: taskID, expectedName: expectedName, name: name, at: occurredAt))
    }
}
"""
CONTRACT_BEFORE = """public protocol TrackerClient: Sendable {
    func archiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult
    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult
    func renameTask(taskID: String, name: String, occurredAt: String) async throws -> TaskCommandResult
}
"""
CONTRACT_AFTER = """public protocol TrackerClient: Sendable {
    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult
    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult
    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws
        -> TaskCommandResult
}
"""


def apply_edits(text, edits):
    for old, new in edits:
        text = text.replace(old, new)
    return text


DELETION_SUPPORT_BEFORE = (
    "import Foundation\n@MainActor\nfinal class FakeClient: TrackerClient {\n    enum Operation: Equatable {\n"
    + "".join(old if old.endswith("\n") else old + "\n" for old, _ in scope.WORKLOG_DELETION_SUPPORT_EDITS)
    + "    }\n}\n"
)
DELETION_SUPPORT_AFTER = apply_edits(DELETION_SUPPORT_BEFORE, scope.WORKLOG_DELETION_SUPPORT_EDITS)
DELETION_CONTRACT_BEFORE = (
    "public protocol TrackerClient: Sendable {\n"
    + "".join(old for old, _ in scope.WORKLOG_DELETION_CONTRACT_EDITS)
    + "}\n"
)
DELETION_CONTRACT_AFTER = apply_edits(DELETION_CONTRACT_BEFORE, scope.WORKLOG_DELETION_CONTRACT_EDITS)


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

    def reviewed_name_baseline(self):
        self.write(scope.PACKAGE + SUPPORT, SUPPORT_BEFORE)
        self.write(scope.PACKAGE + CONTRACT, CONTRACT_BEFORE)
        for test in (RENAME_TEST, ARCHIVING_TEST):
            self.write(scope.PACKAGE + test, "func testTaskCommand() {}\n")
        self.base = self.commit("Add shared client fixture")

    def reviewed_name_changes(self):
        self.write(scope.PACKAGE + SUPPORT, SUPPORT_AFTER)
        self.write(scope.PACKAGE + CONTRACT, CONTRACT_AFTER)
        for test in (RENAME_TEST, ARCHIVING_TEST):
            self.write(scope.PACKAGE + test, "func testTaskCommandRetainsReviewedName() {}\n")

    def test_reviewed_name_fixture_adaptation_keeps_the_complete_focused_scope(self):
        self.reviewed_name_baseline()
        self.reviewed_name_changes()
        self.commit("Forward reviewed names through the client contract and fixture")

        result = self.select()

        expected = {CONTRACT}
        for test in (RENAME_TEST, ARCHIVING_TEST):
            expected.update(scope.SOURCES + source for source in scope.TEST_PRODUCTION[test.removeprefix(scope.TESTS)])
        self.assertEqual(result["status"], "focused", result["review_reasons"])
        self.assertEqual(result["files"], sorted(expected))
        self.assertEqual(len(result["files"]), 14)
        self.assertEqual(result["review_reasons"], [])
        self.assertTrue(any("Shared fixture only forwards reviewed names" in reason for reason in result["reasons"]))

    def test_reviewed_name_adaptation_requires_both_contract_and_mapped_suites(self):
        self.reviewed_name_baseline()
        for missing in (CONTRACT, RENAME_TEST, ARCHIVING_TEST):
            with self.subTest(missing=missing):
                self.git("reset", "--hard", self.base)
                self.reviewed_name_changes()
                self.git("restore", scope.PACKAGE + missing)
                self.commit("Omit a required reviewed-name change")
                self.assertEqual(self.select()["status"], "review-required")

    def test_reviewed_name_fixture_rejects_incomplete_forwarding_and_unknown_shapes(self):
        self.reviewed_name_baseline()
        variants = {
            "archive forwarding": SUPPORT_AFTER.replace(
                ".archive(task: taskID, expectedName: expectedName, at: occurredAt)",
                ".archive(task: taskID, at: occurredAt)"),
            "rename forwarding": SUPPORT_AFTER.replace(
                ".rename(task: taskID, expectedName: expectedName, name: name, at: occurredAt)",
                ".rename(task: taskID, expectedName: name, name: name, at: occurredAt)"),
            "archive declaration": SUPPORT_AFTER.replace(
                "case archive(task: String, expectedName: String, at: String)",
                "case archive(task: String, at: String)"),
            "rename declaration": SUPPORT_AFTER.replace(
                "case rename(task: String, expectedName: String, name: String, at: String)",
                "case rename(task: String, expectedName: String, name: String, at: String, extra: Bool)"),
            "outer destructuring": SUPPORT_AFTER.replace(
                ".rename(let id, _, _, _), .archive(let id, _, _)",
                ".rename(let id, _, _), .archive(let id, _)"),
            "inner destructuring": SUPPORT_AFTER.replace(
                "case .rename(_, _, let name, _):", "case .rename(_, let name, _):"),
            "archive signature": SUPPORT_AFTER.replace(
                "func archiveTask(taskID: String, expectedName: String, occurredAt: String)",
                "func archiveTask(taskID: String, occurredAt: String)"),
            "rename signature": SUPPORT_AFTER.replace(
                "func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String)",
                "func renameTask(taskID: String, name: String, occurredAt: String)"),
        }
        for label, content in variants.items():
            with self.subTest(change=label):
                self.git("reset", "--hard", self.base)
                self.reviewed_name_changes()
                self.write(scope.PACKAGE + SUPPORT, content)
                self.commit("Change the reviewed-name fixture shape")
                self.assertEqual(self.select()["status"], "review-required")

    def test_reviewed_name_adaptation_rejects_other_shared_fixture_behavior_changes(self):
        self.reviewed_name_baseline()
        variants = {
            "reply": SUPPORT_AFTER.replace("receipt: nil", "receipt: resources.receipt"),
            "scheduling": SUPPORT_AFTER.replace("var requests: [Request] = []", "var requests: [Request] = cachedRequests"),
            "lifecycle": SUPPORT_AFTER.replace("requests.removeAll()", "requests.removeFirst()"),
            "whitespace": SUPPORT_AFTER.replace("import Foundation\n", "import Foundation\n\n"),
            "extra field": SUPPORT_AFTER.replace("enum Reply {", "let expectedName = \"Unexpected\"\n    enum Reply {"),
            "duplicate known shape": SUPPORT_AFTER.replace(
                "        case archive(task: String, expectedName: String, at: String)\n",
                "        case archive(task: String, expectedName: String, at: String)\n" * 2),
        }
        for label, content in variants.items():
            with self.subTest(change=label):
                self.git("reset", "--hard", self.base)
                self.reviewed_name_changes()
                self.write(scope.PACKAGE + SUPPORT, content)
                self.commit("Change unrelated shared fixture content")
                self.assertEqual(self.select()["status"], "review-required")

    def test_reviewed_name_adaptation_rejects_extra_contract_changes(self):
        self.reviewed_name_baseline()
        variants = {
            "unarchive": CONTRACT_AFTER.replace(
                "func unarchiveTask(taskID: String, occurredAt: String)",
                "func unarchiveTask(taskID: String, expectedName: String, occurredAt: String)"),
            "missing archive guard": CONTRACT_AFTER.replace(
                "func archiveTask(taskID: String, expectedName: String, occurredAt: String)",
                "func archiveTask(taskID: String, occurredAt: String)"),
            "whitespace": CONTRACT_AFTER.replace("public protocol", "\npublic protocol"),
        }
        for label, content in variants.items():
            with self.subTest(change=label):
                self.git("reset", "--hard", self.base)
                self.reviewed_name_changes()
                self.write(scope.PACKAGE + CONTRACT, content)
                self.commit("Change the reviewed-name contract shape")
                self.assertEqual(self.select()["status"], "review-required")

    def test_reviewed_name_adaptation_does_not_exempt_other_support_or_build_inputs(self):
        self.reviewed_name_baseline()
        for path in (scope.TESTS + "Support/OtherSupport.swift", "Package.swift", "Package.resolved"):
            with self.subTest(path=path):
                self.git("reset", "--hard", self.base)
                self.reviewed_name_changes()
                self.write(scope.PACKAGE + path, "Changed shared input\n")
                self.commit("Also change an unaudited shared input")
                self.assertEqual(self.select()["status"], "review-required")

    def test_reviewed_name_adaptation_rejects_missing_production_sources(self):
        self.reviewed_name_baseline()
        self.reviewed_name_changes()
        self.git("rm", scope.PACKAGE + scope.SOURCES + scope.RENAME)
        self.commit("Remove a production source exercised by the changed tests")

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertTrue(any("Missing, unsafe, or stale production source" in reason
                            for reason in result["review_reasons"]))

    def test_reviewed_name_adaptation_rejects_symlink_and_mode_changes(self):
        self.reviewed_name_baseline()
        for symlink in (False, True):
            with self.subTest(symlink=symlink):
                self.git("reset", "--hard", self.base)
                self.reviewed_name_changes()
                path = self.root / scope.PACKAGE / SUPPORT
                if symlink:
                    path.unlink()
                    path.symlink_to("ReviewedSupport.swift")
                else:
                    path.chmod(0o755)
                self.commit("Change the shared fixture file mode")
                self.assertEqual(self.select()["status"], "review-required")

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

    def deletion_baseline(self):
        self.write(scope.PACKAGE + SUPPORT, DELETION_SUPPORT_BEFORE)
        self.write(scope.PACKAGE + CONTRACT, DELETION_CONTRACT_BEFORE)
        self.base = self.commit("Add shared client fixture")

    def deletion_changes(self):
        self.write(scope.PACKAGE + SUPPORT, DELETION_SUPPORT_AFTER)
        self.write(scope.PACKAGE + CONTRACT, DELETION_CONTRACT_AFTER)
        self.write(scope.PACKAGE + DELETION_TEST, "func testDeletion() {}\n")

    def test_worklog_deletion_fixture_addition_keeps_the_complete_focused_scope(self):
        self.deletion_baseline()
        self.deletion_changes()
        self.commit("Add worklog deletion to the client contract and fixture")

        result = self.select()

        expected = {CONTRACT}
        expected.update(
            scope.SOURCES + source for source in scope.TEST_PRODUCTION[DELETION_TEST.removeprefix(scope.TESTS)])
        self.assertEqual(result["status"], "focused", result["review_reasons"])
        self.assertEqual(result["files"], sorted(expected))
        self.assertIn(scope.SOURCES + scope.DELETION, result["files"])
        self.assertTrue(any("Shared fixture only adds the worklog deletion reply" in reason
                            for reason in result["reasons"]))

    def test_worklog_deletion_fixture_addition_requires_the_contract_and_suite(self):
        self.deletion_baseline()
        for missing in (CONTRACT, DELETION_TEST):
            with self.subTest(missing=missing):
                self.git("reset", "--hard", self.base)
                self.git("clean", "-fd")
                self.deletion_changes()
                if missing == DELETION_TEST:
                    (self.root / scope.PACKAGE / DELETION_TEST).unlink()
                else:
                    self.git("restore", scope.PACKAGE + missing)
                self.commit("Omit a required deletion change")
                self.assertEqual(self.select()["status"], "review-required")

    def test_worklog_deletion_fixture_rejects_other_shared_fixture_and_contract_changes(self):
        self.deletion_baseline()
        variants = {
            SUPPORT: {
                "reply": DELETION_SUPPORT_AFTER.replace("worklog == expected", "worklog.id == expected.id"),
                "operation": DELETION_SUPPORT_AFTER.replace(
                    "case delete(expected: WorklogItem)", "case delete(expected: WorklogItem, at: String)"),
                "extra line": DELETION_SUPPORT_AFTER.replace("    }\n}\n", "    }\n    var extra = 0\n}\n"),
                "missing reply": DELETION_SUPPORT_AFTER.replace("        case deleted(WorklogDeletionResult)\n", ""),
            },
            CONTRACT: {
                "signature": DELETION_CONTRACT_AFTER.replace(
                    "func deleteWorklog(expected: WorklogItem)", "func deleteWorklog(id: String)"),
                "extra requirement": DELETION_CONTRACT_AFTER.replace(
                    "}\n", "    func purgeWorklogs() async throws\n}\n"),
            },
        }
        for path, contents in variants.items():
            for label, content in contents.items():
                with self.subTest(path=path, change=label):
                    self.git("reset", "--hard", self.base)
                    self.git("clean", "-fd")
                    self.deletion_changes()
                    self.write(scope.PACKAGE + path, content)
                    self.commit("Change the deletion fixture shape")
                    self.assertEqual(self.select()["status"], "review-required")

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

    def test_symlink_ancestor_above_the_repository_does_not_reject_regular_sources(self):
        self.write(scope.PACKAGE + TRACKING, "func changedTracking() {}\n")
        head = self.commit("Change production source")
        ancestor = self.root / "ancestor"
        ancestor.symlink_to(Path(self.root.anchor), target_is_directory=True)
        repository_alias = ancestor / self.root.relative_to(self.root.anchor)

        result = scope.select_scope(repository_alias, self.base, head)

        self.assertEqual(result["status"], "focused")
        self.assertEqual(result["files"], [TRACKING])
        self.assertEqual(repository_alias.resolve(), self.root.resolve())

    def test_symlink_directory_inside_the_repository_cannot_escape_the_package(self):
        self.write(scope.PACKAGE + TRACKING, "func changedTracking() {}\n")
        self.commit("Change production source")
        source_directory = (self.root / scope.PACKAGE / TRACKING).parent
        outside = self.root / "outside-tracking"
        source_directory.rename(outside)
        source_directory.symlink_to(outside, target_is_directory=True)

        result = self.select()

        self.assertEqual(result["status"], "review-required")
        self.assertEqual(result["files"], [])

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
