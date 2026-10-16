#!/usr/bin/env python3
"""Select whole portable Swift files from the complete pull request diff."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "apps/swiftui/TrackerClient/"
SOURCES = "Sources/TrackerClient/"
TESTS = "Tests/TrackerClientTests/"
SESSION = "App/TrackerSession.swift"
OBSERVER = "App/TrackerPresentationObserver.swift"
MODELS = "Contracts/Models.swift"
DEPENDENCIES = "Contracts/Dependencies.swift"
TRACKING = "Features/Tracking/TrackingState.swift"
CATALOG = "Features/TaskCatalog/TaskCatalogState.swift"
HISTORY = "Features/WorklogHistory/WorklogHistoryState.swift"
TIMESTAMP = "Features/WorklogHistory/WorklogTimestamp.swift"
TOTALS = "Features/DailyTotals/DailyTotalsState.swift"
MENU = "Features/MenuBar/TrackerMenuContent.swift"
MENU_VALUES = "Features/MenuBar/TrackerMenuValues.swift"
MENU_OBSERVER = "Features/MenuBar/TrackerMenuPresentationObserver.swift"
LAST_TASK = "Features/MenuBar/LastTrackedTaskState.swift"
MENU_ORDER = "Features/MenuBar/MenuTaskOrder.swift"
CONNECTION = "Features/Connection/ConnectionState.swift"
AUTOMATION = "Features/TrackingAutomation/TrackingAutomationState.swift"
CREATION = "Features/TaskCreation/TaskCreationState.swift"
RENAME = "Features/TaskRename/TaskRenameState.swift"
ARCHIVING = "Features/TaskArchiving/TaskArchivingState.swift"
BULK_ARCHIVING = "Features/BulkTaskArchiving/BulkTaskArchivingState.swift"
CORRECTION = "Features/WorklogCorrection/WorklogCorrectionState.swift"
MOVE = "Features/WorklogMove/WorklogMoveState.swift"
INDICATOR = "Features/TaskIndicators/TaskIndicator.swift"

# Map each suite's assertions to complete files, including its session coordinator.
# Update this map when a suite gains assertions about another production component.
TEST_PRODUCTION = {
    "App/BackgroundRefreshInteractionTests.swift": [
        SESSION, "Features/Connection/ConnectionState.swift", TRACKING, CATALOG, HISTORY, TOTALS,
        "Features/TrackingAutomation/TrackingAutomationState.swift",
    ],
    "App/HistoryPresentationTests.swift": [SESSION, OBSERVER, HISTORY, CATALOG],
    "App/LifecycleTests.swift": [
        SESSION, "Features/Connection/ConnectionState.swift", TRACKING, CATALOG, HISTORY, TOTALS,
        "Features/TrackingAutomation/TrackingAutomationState.swift", LAST_TASK,
    ],
    "App/ObserverTests.swift": [SESSION, TRACKING, CATALOG, HISTORY, TOTALS, CONNECTION],
    "App/PresentationObserverTests.swift": [
        SESSION, OBSERVER, TRACKING, CATALOG, TOTALS, CREATION, AUTOMATION, CONNECTION,
    ],
    "App/RefreshPresentationTests.swift": [SESSION, OBSERVER, TRACKING, CATALOG, TOTALS, MODELS, CONNECTION],
    "Contracts/ModelsTests.swift": [MODELS, DEPENDENCIES, TRACKING],
    "Features/BulkTaskArchiving/BulkTaskArchivingTests.swift": [
        SESSION, OBSERVER, BULK_ARCHIVING, CATALOG, TRACKING, CONNECTION, CREATION, RENAME,
        ARCHIVING, CORRECTION, MOVE,
    ],
    "Features/Connection/ConnectionTests.swift": [SESSION, CONNECTION, CATALOG, TRACKING, HISTORY, ARCHIVING],
    "Features/DailyTotals/DailyTotalsTests.swift": [
        SESSION, OBSERVER, TOTALS, TRACKING, MODELS, CONNECTION, CATALOG, HISTORY,
    ],
    "Features/MenuBar/MenuKeyboardShortcutsTests.swift": [
        "Features/MenuBar/MenuKeyboardShortcuts.swift", DEPENDENCIES,
    ],
    "Features/MenuBar/MenuLiveValuesTests.swift": [
        SESSION, MENU_OBSERVER, MENU, MENU_VALUES, TOTALS, TRACKING, CONNECTION, CATALOG, LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuPresentationTests.swift": [
        SESSION, OBSERVER, MENU_OBSERVER, MENU, MENU_ORDER, MENU_VALUES, TOTALS, TRACKING, CATALOG, CONNECTION,
        LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuTaskOrderTests.swift": [
        SESSION, MENU_ORDER, MENU_OBSERVER, MENU, MENU_VALUES, TOTALS, TRACKING, CATALOG, CONNECTION,
        LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuTaskClipboardTests.swift": [
        SESSION, "Features/MenuBar/MenuTaskClipboard.swift", "Features/MenuBar/MenuKeyboardShortcuts.swift",
        MENU_OBSERVER, MENU, MENU_VALUES, TOTALS, TRACKING, CATALOG, CONNECTION, LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuTaskTrackingActionTests.swift": [
        SESSION, "Features/MenuBar/MenuTaskTrackingAction.swift", MENU_OBSERVER, MENU, MENU_VALUES, TRACKING,
        "Features/MenuBar/MenuTaskClipboard.swift", "Features/MenuBar/MenuKeyboardShortcuts.swift",
        CATALOG, CONNECTION, LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/StatusItemTests.swift": [
        SESSION, LAST_TASK, "Features/TaskIndicators/TaskIndicator.swift", MENU_OBSERVER, MENU, MENU_VALUES,
        TRACKING, CATALOG, CONNECTION, AUTOMATION,
    ],
    "Features/ProgressPresentation/DelayedProgressPresentationTests.swift": [
        "Features/ProgressPresentation/DelayedProgressPresentation.swift",
    ],
    "Features/TaskArchiving/TaskArchivingTests.swift": [
        SESSION, OBSERVER, ARCHIVING, CATALOG, TRACKING, HISTORY, CREATION, RENAME, CORRECTION, MOVE, CONNECTION,
    ],
    "Features/TaskCatalog/TaskCatalogTests.swift": [CATALOG],
    "Features/TaskCreation/TaskCreationTests.swift": [
        SESSION, OBSERVER, CREATION, CATALOG, TRACKING, HISTORY, TOTALS, CONNECTION, AUTOMATION,
        "Features/TaskEditing/TaskNameEditingPolicy.swift",
    ],
    "Features/TaskEditing/TaskNameEditingPolicyTests.swift": [
        SESSION, "Features/TaskEditing/TaskNameEditingPolicy.swift", CREATION, RENAME, CATALOG,
    ],
    "Features/TaskRename/TaskRenameTests.swift": [
        SESSION, OBSERVER, RENAME, CATALOG, TRACKING, HISTORY, CREATION, AUTOMATION, CONNECTION,
        "Features/TaskEditing/TaskNameEditingPolicy.swift",
    ],
    "Features/Tracking/TrackingStateTests.swift": [TRACKING],
    "Features/Tracking/TrackingTests.swift": [SESSION, TRACKING, CATALOG, HISTORY, LAST_TASK, CONNECTION],
    "Features/TrackingAutomation/TrackingAutomationTests.swift": [
        SESSION, AUTOMATION, TRACKING, CATALOG, CONNECTION, HISTORY,
    ],
    "Features/WindowRouting/WindowRoutingTests.swift": ["Features/WindowRouting/TrackerWindowRoutingState.swift"],
    "Features/WorklogCorrection/WorklogCorrectionTests.swift": [
        SESSION, OBSERVER, CORRECTION, TIMESTAMP, TRACKING, HISTORY, CATALOG, CONNECTION, CREATION, RENAME,
    ],
    "Features/WorklogHistory/HistoryTests.swift": [SESSION, HISTORY, TRACKING, CATALOG, CONNECTION],
    "Features/WorklogHistory/WorklogHistoryStateTests.swift": [HISTORY],
    "Features/WorklogMove/WorklogMoveTests.swift": [
        SESSION, OBSERVER, MOVE, TIMESTAMP, HISTORY, TRACKING, CATALOG, CONNECTION, CREATION, CORRECTION,
    ],
}


# These exact edits only carry reviewed names through the shared fake client.
# Comparing the remaining bytes keeps reply and scheduling changes under review.
REVIEWED_NAME_SUPPORT = TESTS + "Support/TestSupport.swift"
REVIEWED_NAME_SUPPORT_EDITS = (
    ("        case archive(task: String, at: String)\n",
     "        case archive(task: String, expectedName: String, at: String)\n"),
    ("        case rename(task: String, name: String, at: String)\n",
     "        case rename(task: String, expectedName: String, name: String, at: String)\n"),
    ("            case .rename(let id, _, _), .archive(let id, _), .unarchive(let id, _):\n",
     "            case .rename(let id, _, _, _), .archive(let id, _, _), .unarchive(let id, _):\n"),
    ("                case .rename(_, let name, _):\n",
     "                case .rename(_, _, let name, _):\n"),
    ("    func archiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult {\n"
     "        try await taskReply(.archive(task: taskID, at: occurredAt))\n"
     "    }\n",
     "    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult {\n"
     "        try await taskReply(.archive(task: taskID, expectedName: expectedName, at: occurredAt))\n"
     "    }\n"),
    ("    func renameTask(taskID: String, name: String, occurredAt: String) async throws -> TaskCommandResult {\n"
     "        try await taskReply(.rename(task: taskID, name: name, at: occurredAt))\n"
     "    }\n",
     "    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws\n"
     "        -> TaskCommandResult\n"
     "    {\n"
     "        try await taskReply(.rename(task: taskID, expectedName: expectedName, name: name, at: occurredAt))\n"
     "    }\n"),
)
REVIEWED_NAME_CONTRACT_EDITS = (
    ("    func archiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult\n",
     "    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult\n"),
    ("    func renameTask(taskID: String, name: String, occurredAt: String) async throws -> TaskCommandResult\n",
     "    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws\n"
     "        -> TaskCommandResult\n"),
)


class ScopeError(Exception):
    pass


def git(root: Path, *arguments: str) -> bytes:
    result = subprocess.run(["git", "-C", str(root), *arguments], capture_output=True, check=False)
    if result.returncode:
        raise ScopeError(result.stderr.decode("utf-8", errors="replace").strip() or "Git command failed")
    return result.stdout


def commit(root: Path, revision: str) -> str:
    return git(root, "rev-parse", "--verify", "--end-of-options", revision + "^{commit}").decode().strip()


def changes(root: Path, base: str, head: str) -> list[dict]:
    fields = git(root, "diff", "--no-ext-diff", "--no-textconv", "--name-status", "-z", "--find-renames",
                 base, head, "--").split(b"\0")
    entries = []
    index = 0
    while index < len(fields) - 1:
        status = fields[index].decode("ascii")
        index += 1
        path = fields[index].decode("utf-8", errors="surrogateescape")
        index += 1
        entry = {"status": status, "path": path}
        if status.startswith(("R", "C")):
            entry["old_path"] = path
            entry["path"] = fields[index].decode("utf-8", errors="surrogateescape")
            index += 1
        entries.append(entry)
    return entries


def source_tree(root: Path, head: str) -> dict[str, str]:
    tree = {}
    for record in git(root, "ls-tree", "-r", "-z", head, "--", PACKAGE + SOURCES).split(b"\0"):
        if not record:
            continue
        metadata, path = record.split(b"\t", 1)
        name = path.decode("utf-8", errors="surrogateescape")
        if name.endswith(".swift"):
            tree[name.removeprefix(PACKAGE)] = metadata.split()[0].decode("ascii")
    return tree


def valid_source(root: Path, name: str, tree: dict[str, str]) -> bool:
    path = PurePosixPath(name)
    if (not re.fullmatch(r"[A-Za-z0-9_./-]+", name) or ".." in path.parts or path.is_absolute()
            or path.as_posix() != name or not name.startswith(SOURCES) or not name.endswith(".swift")
            or tree.get(name) not in {"100644", "100755"}):
        return False
    package = root / PACKAGE
    candidate = package / name
    if any(parent.is_symlink() for parent in (candidate, *candidate.parents) if parent.is_relative_to(root)):
        return False
    return candidate.is_file() and candidate.resolve().is_relative_to(package.resolve())


def only_reviewed_name_edits(root: Path, base: str, head: str, path: str, edits: tuple) -> bool:
    contents = []
    for revision in (base, head):
        record = git(root, "ls-tree", "-z", revision, "--", path)
        if not record or record.split()[0] != b"100644":
            return False
        contents.append(git(root, "show", revision + ":" + path))
    before, after = contents
    for old, new in edits:
        old, new = old.encode("utf-8"), new.encode("utf-8")
        if before.count(old) != 1 or after.count(new) != 1:
            return False
        after = after.replace(new, old)
    return after == before


def reviewed_name_support_change(root: Path, base: str, head: str, entries: list[dict]) -> bool:
    modified = {entry["path"] for entry in entries if entry["status"] == "M"}
    required = {
        PACKAGE + REVIEWED_NAME_SUPPORT,
        PACKAGE + SOURCES + DEPENDENCIES,
        PACKAGE + TESTS + "Features/TaskRename/TaskRenameTests.swift",
        PACKAGE + TESTS + "Features/TaskArchiving/TaskArchivingTests.swift",
    }
    if not required.issubset(modified):
        return False
    return (
        only_reviewed_name_edits(root, base, head, PACKAGE + REVIEWED_NAME_SUPPORT,
                                 REVIEWED_NAME_SUPPORT_EDITS)
        and only_reviewed_name_edits(root, base, head, PACKAGE + SOURCES + DEPENDENCIES,
                                    REVIEWED_NAME_CONTRACT_EDITS)
    )


def select_scope(root: Path, base: str, head: str) -> dict:
    base = commit(root, base)
    head = commit(root, head)
    if commit(root, "HEAD") != head:
        raise ScopeError("Check out the requested head commit before selecting mutation files")
    merge_base = git(root, "merge-base", base, head).decode().strip()
    entries = changes(root, merge_base, head)
    tree = source_tree(root, head)
    reviewed_names_only = reviewed_name_support_change(root, merge_base, head, entries)
    renamed = {
        entry["old_path"].removeprefix(PACKAGE): entry["path"].removeprefix(PACKAGE)
        for entry in entries if entry["status"].startswith("R")
        and entry["old_path"].startswith(PACKAGE + SOURCES) and entry["path"].startswith(PACKAGE + SOURCES)
    }
    selected = set()
    reasons = []
    review = []

    def add_source(name: str, reason: str) -> None:
        name = renamed.get(name, name)
        if not valid_source(root, name, tree):
            review.append(f"Missing, unsafe, or stale production source: {name}")
        else:
            selected.add(name)
            reasons.append(reason)

    for entry in entries:
        paths = [entry["path"]]
        if "old_path" in entry:
            paths.append(entry["old_path"])
        for path in dict.fromkeys(paths):
            relative = path.removeprefix(PACKAGE)
            if path.endswith((".md", ".html")):
                reasons.append(f"Documentation does not change portable production scope: {path}")
            elif path.startswith(PACKAGE + SOURCES):
                if entry["status"] == "D" or path == entry.get("old_path"):
                    if path == entry.get("old_path") and relative in renamed:
                        continue
                    review.append(f"Deleted or moved-out production source needs scope review: {path}")
                else:
                    add_source(relative, f"Changed production file: {path}")
            elif path.startswith(PACKAGE + TESTS):
                test = relative.removeprefix(TESTS)
                if relative == REVIEWED_NAME_SUPPORT and reviewed_names_only:
                    reasons.append(f"Shared fixture only forwards reviewed names; changed rename/archive suites define scope: {path}")
                elif test not in TEST_PRODUCTION:
                    review.append(f"Unmapped test or shared test support needs scope review: {path}")
                else:
                    for source in TEST_PRODUCTION[test]:
                        add_source(SOURCES + source, f"Production exercised by changed test: {path}")
            elif path.startswith(PACKAGE):
                review.append(f"Portable package build or dependency input needs scope review: {path}")
            elif path in {"Package.swift", "Package.resolved", ".swift-version"} or path.startswith(".swiftpm/"):
                review.append(f"Shared Swift build or dependency input needs scope review: {path}")
            else:
                reasons.append(f"No portable production scope for documentation, tooling, or other components: {path}")

    if review:
        status = "review-required"
    elif selected:
        status = "focused"
    else:
        status = "empty"
        reasons.append("No changed portable production files or mapped portable tests in the pull request diff")
    return {"status": status, "base": base, "head": head, "merge_base": merge_base,
            "files": sorted(selected), "changes": entries, "reasons": sorted(set(reasons)),
            "review_reasons": sorted(set(review))}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="Pull request base commit or reference")
    parser.add_argument("--head", required=True, help="Checked-out pull request head commit or reference")
    parser.add_argument("--output", type=Path, required=True, help="Scope JSON artifact")
    arguments = parser.parse_args()
    try:
        scope = select_scope(ROOT, arguments.base, arguments.head)
    except (ScopeError, OSError, UnicodeError, ValueError) as error:
        scope = {"status": "review-required", "files": [], "review_reasons": [str(error)]}
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(json.dumps(scope, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    print(f"Swift mutation scope: {scope['status']}, {len(scope['files'])} complete production files")
    return 2 if scope["status"] == "review-required" else 0


if __name__ == "__main__":
    sys.exit(main())
