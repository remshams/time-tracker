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
PRESENTATION = "App/TrackerPresentation.swift"
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
MENU_SHORTCUTS = "Features/MenuBar/MenuKeyboardShortcuts.swift"
MENU_CLIPBOARD = "Features/MenuBar/MenuTaskClipboard.swift"
CONNECTION = "Features/Connection/ConnectionState.swift"
AUTOMATION = "Features/TrackingAutomation/TrackingAutomationState.swift"
CREATION = "Features/TaskCreation/TaskCreationState.swift"
RENAME = "Features/TaskRename/TaskRenameState.swift"
NAME_POLICY = "Features/TaskEditing/TaskNameEditingPolicy.swift"
ARCHIVING = "Features/TaskArchiving/TaskArchivingState.swift"
BULK_ARCHIVING = "Features/BulkTaskArchiving/BulkTaskArchivingState.swift"
CORRECTION = "Features/WorklogCorrection/WorklogCorrectionState.swift"
MOVE = "Features/WorklogMove/WorklogMoveState.swift"
DELETION = "Features/WorklogDeletion/WorklogDeletionState.swift"
INDICATOR = "Features/TaskIndicators/TaskIndicator.swift"
TIME_FORMATTING = "Shared/TimeFormatting.swift"

# Map each suite's assertions to complete files, including its session coordinator.
# Update this map when a suite gains assertions about another production component.
TEST_PRODUCTION = {
    "App/BackgroundRefreshInteractionTests.swift": [
        SESSION, CONNECTION, TRACKING, TIME_FORMATTING, CATALOG, HISTORY, TOTALS, AUTOMATION, CREATION,
    ],
    "App/HistoryPresentationTests.swift": [SESSION, PRESENTATION, HISTORY, CATALOG],
    "App/LifecycleTests.swift": [
        SESSION, CONNECTION, TRACKING, TIME_FORMATTING, CATALOG, HISTORY, TOTALS, AUTOMATION, LAST_TASK,
    ],
    "App/ObserverTests.swift": [SESSION, TRACKING, TIME_FORMATTING, CATALOG, HISTORY, TOTALS, CONNECTION],
    "App/PresentationTests.swift": [
        SESSION, PRESENTATION, TRACKING, TIME_FORMATTING, CATALOG, TOTALS, CREATION, AUTOMATION, CONNECTION,
    ],
    "App/RefreshPresentationTests.swift": [
        SESSION, PRESENTATION, TRACKING, TIME_FORMATTING, CATALOG, TOTALS, MODELS, CONNECTION,
    ],
    "Contracts/ModelsTests.swift": [MODELS, DEPENDENCIES, TIME_FORMATTING],
    "Features/BulkTaskArchiving/BulkTaskArchivingTests.swift": [
        SESSION, PRESENTATION, BULK_ARCHIVING, CATALOG, TRACKING, TIME_FORMATTING, CONNECTION, CREATION, RENAME,
        ARCHIVING, CORRECTION, MOVE,
    ],
    "Features/Connection/ConnectionTests.swift": [
        SESSION, CONNECTION, CATALOG, TRACKING, TIME_FORMATTING, HISTORY, ARCHIVING,
    ],
    "Features/DailyTotals/DailyTotalsTests.swift": [
        SESSION, PRESENTATION, TOTALS, TRACKING, TIME_FORMATTING, MODELS, CONNECTION, CATALOG, HISTORY,
    ],
    "Features/MenuBar/MenuKeyboardShortcutsTests.swift": [
        MENU_SHORTCUTS, DEPENDENCIES,
    ],
    "Features/MenuBar/MenuLiveValuesTests.swift": [
        SESSION, MENU_OBSERVER, MENU, MENU_VALUES, TOTALS, TRACKING, TIME_FORMATTING, CONNECTION, CATALOG, LAST_TASK,
        INDICATOR,
    ],
    "Features/MenuBar/MenuPresentationTests.swift": [
        SESSION, PRESENTATION, MENU_OBSERVER, MENU, MENU_ORDER, MENU_VALUES, TOTALS, TRACKING, TIME_FORMATTING,
        CATALOG, CONNECTION, LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuTaskOrderTests.swift": [
        SESSION, MENU_ORDER, MENU_OBSERVER, MENU, MENU_VALUES, TOTALS, TRACKING, TIME_FORMATTING, CATALOG, CONNECTION,
        LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuTaskClipboardTests.swift": [
        SESSION, MENU_CLIPBOARD, MENU_SHORTCUTS,
        MENU_OBSERVER, MENU, MENU_VALUES, TOTALS, TRACKING, TIME_FORMATTING, CATALOG, CONNECTION, LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/MenuTaskTrackingActionTests.swift": [
        SESSION, "Features/MenuBar/MenuTaskTrackingAction.swift", MENU_OBSERVER, MENU, MENU_VALUES, TRACKING,
        TIME_FORMATTING, MENU_CLIPBOARD, MENU_SHORTCUTS,
        CATALOG, CONNECTION, LAST_TASK, INDICATOR,
    ],
    "Features/MenuBar/StatusItemTests.swift": [
        SESSION, LAST_TASK, INDICATOR, MENU_OBSERVER, MENU, MENU_VALUES,
        TRACKING, TIME_FORMATTING, CATALOG, CONNECTION, AUTOMATION,
    ],
    "Features/ProgressPresentation/DelayedProgressPresentationTests.swift": [
        "Features/ProgressPresentation/DelayedProgressPresentation.swift",
    ],
    "Features/TaskArchiving/TaskArchivingTests.swift": [
        SESSION, PRESENTATION, ARCHIVING, CATALOG, TRACKING, TIME_FORMATTING, HISTORY, CREATION, RENAME, CORRECTION,
        MOVE, CONNECTION,
    ],
    "Features/TaskCatalog/TaskCatalogTests.swift": [CATALOG],
    "Features/TaskCreation/TaskCreationTests.swift": [
        SESSION, PRESENTATION, CREATION, CATALOG, TRACKING, TIME_FORMATTING, HISTORY, TOTALS, CONNECTION, AUTOMATION,
        RENAME, NAME_POLICY,
    ],
    "Features/TaskEditing/TaskNameEditingPolicyTests.swift": [
        SESSION, NAME_POLICY, CREATION, RENAME, CATALOG,
    ],
    "Features/TaskRename/TaskRenameTests.swift": [
        SESSION, PRESENTATION, RENAME, CATALOG, TRACKING, TIME_FORMATTING, HISTORY, CREATION, AUTOMATION, CONNECTION,
        NAME_POLICY,
    ],
    "Features/Tracking/TrackingStateTests.swift": [TRACKING, TIME_FORMATTING],
    "Features/Tracking/TrackingTests.swift": [
        SESSION, TRACKING, TIME_FORMATTING, CATALOG, HISTORY, LAST_TASK, CONNECTION,
    ],
    "Features/TrackingAutomation/TrackingAutomationTests.swift": [
        SESSION, AUTOMATION, TRACKING, TIME_FORMATTING, CATALOG, CONNECTION, HISTORY, CREATION,
    ],
    "Features/WindowRouting/WindowRoutingTests.swift": ["Features/WindowRouting/TrackerWindowRoutingState.swift"],
    "Features/WorklogCorrection/WorklogCorrectionTests.swift": [
        SESSION, PRESENTATION, CORRECTION, TIMESTAMP, TRACKING, TIME_FORMATTING, HISTORY, CATALOG, CONNECTION,
        CREATION, RENAME,
    ],
    "Features/WorklogHistory/HistoryTests.swift": [SESSION, HISTORY, TRACKING, TIME_FORMATTING, CATALOG, CONNECTION],
    "Features/WorklogHistory/WorklogHistoryStateTests.swift": [HISTORY],
    "Features/WorklogMove/WorklogMoveTests.swift": [
        SESSION, PRESENTATION, MOVE, TIMESTAMP, HISTORY, TRACKING, TIME_FORMATTING, CATALOG, CONNECTION, CREATION,
        CORRECTION,
    ],
    "Features/WorklogDeletion/WorklogDeletionTests.swift": [
        SESSION, PRESENTATION, DELETION, TIMESTAMP, HISTORY, TRACKING, TIME_FORMATTING, CATALOG, CONNECTION, CREATION,
        RENAME, CORRECTION, ARCHIVING,
    ],
    "Shared/TimeFormattingTests.swift": [TIME_FORMATTING],
    "Support/FixtureCleanupTests.swift": [SESSION, TRACKING, TIME_FORMATTING, CATALOG, HISTORY, CONNECTION],
}


SHARED_TEST_SUPPORT = TESTS + "Support/TestSupport.swift"
# A PR that edits the shared fake client can add a reviewed entry here: (reason,
# exact fixture edits or None, (base blob, head blob) or None, exact contract
# edits or None when the client contract must stay unchanged, required changed
# suites, suites whose production files the fixture edits affect). Remove the
# entry once its PR has merged, since its base snapshot no longer matches main.
REVIEWED_SUPPORT_ADAPTATIONS: tuple = ()
# Test support with its own assertions selects the production files it asserts.
SUPPORT_PRODUCTION = {
    "Support/ObservationProbe.swift": [PRESENTATION, SESSION],
}
# A PR that deletes a file and adds its successor in the same diff can map the
# deleted path to the successor here. The successor's own scope covers the
# deleted file's role. Remove the entry once its PR has merged, since main no
# longer has the deleted file.
REVIEWED_REPLACEMENTS: dict[str, str] = {}
# A PR that edits the package manifest can add a reviewed entry here: (reason,
# exact text edits, production files whose behavior depends on the edit). Remove
# the entry once its PR has merged, since its base text no longer matches main.
REVIEWED_PACKAGE_EDITS: tuple = ()


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


def only_reviewed_edits(root: Path, base: str, head: str, path: str, edits: tuple) -> bool:
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


def only_reviewed_blobs(root: Path, base: str, head: str, path: str, blobs: tuple[str, str]) -> bool:
    """Match both reviewed whole-file snapshots and reject non-regular files."""
    for revision, blob in zip((base, head), blobs, strict=True):
        record = git(root, "ls-tree", "-z", revision, "--", path)
        if not record or record.split(b"\t", 1)[0] != f"100644 blob {blob}".encode("ascii"):
            return False
    return True


def reviewed_support_adaptation(
    root: Path, base: str, head: str, entries: list[dict]
) -> tuple[str, tuple[str, ...]] | None:
    """Return the reason and affected sources for an exact shared-fixture adaptation."""
    modified = {entry["path"] for entry in entries if entry["status"] == "M"}
    changed_suites = {entry["path"] for entry in entries if entry["status"] in {"A", "M"}}
    if PACKAGE + SHARED_TEST_SUPPORT not in modified:
        return None
    contract_changed = PACKAGE + SOURCES + DEPENDENCIES in modified
    for reason, support_edits, support_blobs, contract_edits, suites, affected_suites in REVIEWED_SUPPORT_ADAPTATIONS:
        if (
            {PACKAGE + TESTS + suite for suite in suites}.issubset(changed_suites)
            and (only_reviewed_edits(root, base, head, PACKAGE + SHARED_TEST_SUPPORT, support_edits)
                 if support_edits is not None else
                 only_reviewed_blobs(root, base, head, PACKAGE + SHARED_TEST_SUPPORT, support_blobs))
            and (not contract_changed if contract_edits is None else
                 contract_changed
                 and only_reviewed_edits(root, base, head, PACKAGE + SOURCES + DEPENDENCIES, contract_edits))
        ):
            sources = tuple(sorted({source for suite in affected_suites for source in TEST_PRODUCTION[suite]}))
            return reason, sources
    return None


def reviewed_package_edit(root: Path, base: str, head: str) -> tuple[str, tuple[str, ...]] | None:
    """Return the reason and production scope for a reviewed package manifest edit, if one applies."""
    for reason, edits, sources in REVIEWED_PACKAGE_EDITS:
        if only_reviewed_edits(root, base, head, PACKAGE + "Package.swift", edits):
            return reason, sources
    return None


def select_scope(root: Path, base: str, head: str) -> dict:
    base = commit(root, base)
    head = commit(root, head)
    if commit(root, "HEAD") != head:
        raise ScopeError("Check out the requested head commit before selecting mutation files")
    merge_base = git(root, "merge-base", base, head).decode().strip()
    entries = changes(root, merge_base, head)
    tree = source_tree(root, head)
    reviewed_support = reviewed_support_adaptation(root, merge_base, head, entries)
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

    added = {entry["path"] for entry in entries if entry["status"] == "A" or entry["status"].startswith("R")}
    for entry in entries:
        successor = REVIEWED_REPLACEMENTS.get(entry["path"].removeprefix(PACKAGE))
        if entry["status"] == "D" and successor and PACKAGE + successor in added:
            reasons.append(f"Reviewed replacement by {PACKAGE + successor}: {entry['path']}")
            continue
        paths = [entry["path"]]
        replaced = REVIEWED_REPLACEMENTS.get(entry.get("old_path", "").removeprefix(PACKAGE))
        if replaced and entry["path"] == PACKAGE + replaced:
            reasons.append(f"Reviewed replacement by {entry['path']}: {entry['old_path']}")
        elif "old_path" in entry:
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
                if relative == SHARED_TEST_SUPPORT and reviewed_support:
                    reason, sources = reviewed_support
                    for source in sources:
                        add_source(SOURCES + source, f"{reason}: {path}")
                elif test in SUPPORT_PRODUCTION:
                    for source in SUPPORT_PRODUCTION[test]:
                        add_source(SOURCES + source, f"Production asserted by changed test support: {path}")
                elif test not in TEST_PRODUCTION:
                    review.append(f"Unmapped test or shared test support needs scope review: {path}")
                else:
                    for source in TEST_PRODUCTION[test]:
                        add_source(SOURCES + source, f"Production exercised by changed test: {path}")
            elif path == PACKAGE + "Package.swift" and (package_edit := reviewed_package_edit(root, merge_base, head)):
                reason, sources = package_edit
                for source in sources:
                    add_source(SOURCES + source, f"{reason}: {path}")
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
