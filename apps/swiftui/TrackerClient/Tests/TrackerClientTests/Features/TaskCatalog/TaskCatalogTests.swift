import XCTest
@testable import TrackerClient

final class TaskCatalogTests: XCTestCase {
    @MainActor
    func testRememberedRestoredTaskIgnoresMissingAndArchivedTasks() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        XCTAssertTrue(state.switchTab(to: .archived))
        state.rememberRestoredTask(secondTask.id)
        state.rememberRestoredTask("missing-task")
        state.rememberRestoredTask(archivedTask.id)

        XCTAssertTrue(state.switchTab(to: .active))
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testChangingSelectedWorklogStartReloadsHistoryWithoutChangingItsID() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: activeWorklog))
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: firstTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)

        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: activeWorklog, active: corrected))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testReplacingSelectedWorklogReloadsHistoryEvenWhenStartIsUnchanged() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: activeWorklog))
        let replacement = WorklogItem(
            id: "replacement-worklog", taskId: firstTask.id,
            start: activeWorklog.start, end: nil)

        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: activeWorklog, active: replacement))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testSelectedHistoryReloadsWhenTrackingStartsStopsOrMovesToAnotherTask() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        let other = WorklogItem(
            id: "other-worklog", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)

        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: activeWorklog))
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: activeWorklog, active: nil))
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: activeWorklog))
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: activeWorklog, active: other))
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: other, active: activeWorklog))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testUnrelatedTrackingChangesDoNotReloadSelectedHistory() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        let previous = WorklogItem(
            id: "other-worklog", taskId: secondTask.id,
            start: activeWorklog.start, end: nil)
        let replacement = WorklogItem(
            id: "new-other-worklog", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)

        XCTAssertFalse(state.acceptTaskList(tasks, previousActive: nil, active: previous))
        XCTAssertFalse(state.acceptTaskList(tasks, previousActive: previous, active: replacement))
        XCTAssertFalse(state.acceptTaskList(tasks, previousActive: replacement, active: nil))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testStableCatalogAndWorklogDoNotReloadHistory() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: activeWorklog))

        XCTAssertFalse(state.acceptTaskList(tasks, previousActive: activeWorklog, active: activeWorklog))
        XCTAssertEqual(state.selectedTask, firstTask)
        XCTAssertEqual(state.visibleTasks, tasks)
    }

    @MainActor
    func testLatestStartReloadsOnlyTheSelectedTaskHistory() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask], previousActive: nil, active: nil))
        let updatedOther = TaskItem(
            id: secondTask.id, name: secondTask.name, archived: false,
            latestStart: "2025-01-01T00:00:00.000Z")
        let updatedSelected = TaskItem(
            id: firstTask.id, name: firstTask.name, archived: false,
            latestStart: "2025-01-01T01:00:00.000Z")

        XCTAssertFalse(state.acceptTaskList([firstTask, updatedOther], previousActive: nil, active: nil))
        XCTAssertTrue(state.acceptTaskList([updatedSelected, updatedOther], previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTask, updatedSelected)
        XCTAssertTrue(state.acceptTaskList([firstTask, updatedOther], previousActive: nil, active: nil))
    }

    @MainActor
    func testNilUnknownHiddenAndRepeatedSelectionsLeaveCatalogUnchanged() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))

        XCTAssertFalse(state.switchSelection(to: nil))
        XCTAssertFalse(state.switchSelection(to: "missing-task"))
        XCTAssertFalse(state.switchSelection(to: archivedTask.id))
        XCTAssertFalse(state.switchSelection(to: firstTask.id))
        XCTAssertFalse(state.switchTab(to: .active))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
        XCTAssertTrue(state.switchSelection(to: secondTask.id))
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
        XCTAssertTrue(state.switchTab(to: .archived))
        XCTAssertFalse(state.switchSelection(to: firstTask.id))
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)
        XCTAssertEqual(state.visibleTasks, [archivedTask])
    }

    @MainActor
    func testEachTabRestoresItsSelectionAndFallsBackWhenThatTaskDisappears() async {
        let state = TaskCatalogStore()
        let otherArchived = TaskItem(
            id: "other-archived-task", name: "Other archived task",
            archived: true, latestStart: nil)
        let tasks = [firstTask, secondTask, archivedTask, otherArchived]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.switchSelection(to: secondTask.id))
        XCTAssertTrue(state.switchTab(to: .archived))
        XCTAssertTrue(state.switchSelection(to: otherArchived.id))
        XCTAssertTrue(state.switchTab(to: .active))
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
        XCTAssertTrue(state.switchTab(to: .archived))
        XCTAssertEqual(state.selectedTaskID, otherArchived.id)
        XCTAssertTrue(state.switchTab(to: .active))

        XCTAssertTrue(state.acceptTaskList([firstTask, archivedTask], previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
        XCTAssertTrue(state.switchTab(to: .archived))
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)
        XCTAssertTrue(state.acceptTaskList([firstTask], previousActive: nil, active: nil))
        XCTAssertNil(state.selectedTaskID)
        XCTAssertNil(state.selectedTask)
        XCTAssertTrue(state.visibleTasks.isEmpty)
    }

    @MainActor
    func testUserTabAndSelectionChangesReloadTheHistoryOnlyWhenTheyChangeSomething() async {
        let state = TaskCatalogStore()
        let reloads = HistoryReloads()
        state.reloadHistory = { reloads.count += 1 }
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))

        state.select(firstTask.id)
        state.select(nil)
        state.select(archivedTask.id)
        state.changeTab(.active)
        XCTAssertEqual(reloads.count, 0)
        XCTAssertEqual(state.selectedTaskID, firstTask.id)

        state.select(secondTask.id)
        XCTAssertEqual(reloads.count, 1)
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
        state.changeTab(.archived)
        XCTAssertEqual(reloads.count, 2)
        XCTAssertEqual(state.tab, .archived)
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)
    }

    @MainActor
    func testShowingACreatedTaskReloadsTheHistoryOnlyWhenTheTabOrSelectionChanges() async {
        let state = TaskCatalogStore()
        let reloads = HistoryReloads()
        state.reloadHistory = { reloads.count += 1 }
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))

        state.showTask(firstTask)
        XCTAssertEqual(reloads.count, 0)
        XCTAssertEqual(state.tab, .active)
        XCTAssertEqual(state.selectedTaskID, firstTask.id)

        state.showTask(secondTask)
        XCTAssertEqual(reloads.count, 1, "A new selection in the same tab reloads the history.")
        XCTAssertEqual(state.tab, .active)
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testShowingACreatedTaskTakesItsTabFromTheCatalogAndOtherwiseFromTheCreatedTask() async {
        let state = TaskCatalogStore()
        let reloads = HistoryReloads()
        state.reloadHistory = { reloads.count += 1 }
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        let outdated = TaskItem(id: archivedTask.id, name: archivedTask.name, archived: false, latestStart: nil)

        state.showTask(outdated)
        XCTAssertEqual(reloads.count, 1)
        XCTAssertEqual(state.tab, .archived, "The catalog's copy of the task decides the tab.")
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)

        let missing = TaskItem(id: "missing-task", name: "Missing task", archived: false, latestStart: nil)
        state.showTask(missing)
        XCTAssertEqual(reloads.count, 2)
        XCTAssertEqual(state.tab, .active, "Without a catalog copy, the created task decides the tab.")
        XCTAssertEqual(state.selectedTaskID, firstTask.id, "The tab restores its remembered selection.")
    }

    @MainActor
    func testAcceptedConnectionClearsBothTabMemoriesBeforeLoadingItsTaskList() async {
        let state = TaskCatalogStore()
        let otherArchived = TaskItem(
            id: "other-archived-task", name: "Other archived task",
            archived: true, latestStart: nil)
        let tasks = [firstTask, secondTask, archivedTask, otherArchived]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.switchSelection(to: secondTask.id))
        XCTAssertTrue(state.switchTab(to: .archived))
        XCTAssertTrue(state.switchSelection(to: otherArchived.id))

        state.acceptConnection()
        XCTAssertNil(state.selectedTaskID)
        XCTAssertEqual(state.tab, .archived)
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)
        XCTAssertTrue(state.switchTab(to: .active))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testRestoredTaskDoesNotReplaceTheFallbackSelectionRememberedForItsTab() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask, archivedTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.switchSelection(to: secondTask.id))
        XCTAssertTrue(state.acceptTaskList([firstTask, archivedTask], previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
        XCTAssertTrue(state.switchTab(to: .archived))

        XCTAssertFalse(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.switchTab(to: .active))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testUnchangedCatalogAndRepeatedSelectionsNotifyNoReader() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask, archivedTask]
        XCTAssertTrue(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        let probes = CatalogProbes(state)

        XCTAssertFalse(state.acceptTaskList(tasks, previousActive: nil, active: nil))
        XCTAssertFalse(state.switchSelection(to: firstTask.id))
        XCTAssertFalse(state.switchTab(to: .active))
        state.rememberRestoredTask(secondTask.id)
        XCTAssertEqual(probes.changed(), [])
    }

    @MainActor
    func testAnotherTaskChangeNotifiesOnlyTheReadersOfItsList() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        let probes = CatalogProbes(state)
        let renamedArchived = TaskItem(
            id: archivedTask.id, name: "Renamed archived task", archived: true, latestStart: nil)
        let updatedOther = TaskItem(
            id: secondTask.id, name: secondTask.name, archived: false,
            latestStart: "2025-01-01T00:00:00.000Z")

        XCTAssertFalse(
            state.acceptTaskList([firstTask, secondTask, renamedArchived], previousActive: nil, active: nil))
        XCTAssertEqual(probes.changed(), ["tasks"])
        XCTAssertFalse(
            state.acceptTaskList([firstTask, updatedOther, renamedArchived], previousActive: nil, active: nil))
        XCTAssertEqual(probes.changed(), ["tasks", "visibleTasks"])
        XCTAssertEqual(state.visibleTasks, [firstTask, updatedOther])
    }

    @MainActor
    func testSelectionAndTabChangesNotifyTheirReaders() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        let probes = CatalogProbes(state)

        XCTAssertTrue(state.switchSelection(to: secondTask.id))
        XCTAssertEqual(probes.changed(), ["selectedTaskID", "selectedTask"])
        XCTAssertEqual(state.selectedTask, secondTask)
        XCTAssertTrue(state.switchTab(to: .archived))
        XCTAssertEqual(probes.changed(), ["tab", "visibleTasks", "selectedTaskID", "selectedTask"])
        XCTAssertEqual(state.visibleTasks, [archivedTask])
        XCTAssertEqual(state.selectedTask, archivedTask)
        state.acceptConnection()
        XCTAssertEqual(probes.changed(), ["selectedTaskID", "selectedTask"])
        XCTAssertNil(state.selectedTask)
        XCTAssertEqual(state.tab, .archived)
    }

    @MainActor
    func testSelectedTaskChangeNotifiesItsReadersWithoutChangingTheSelection() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.acceptTaskList([firstTask, secondTask], previousActive: nil, active: nil))
        let probes = CatalogProbes(state)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)

        XCTAssertFalse(state.acceptTaskList([renamed, secondTask], previousActive: nil, active: nil))
        XCTAssertEqual(probes.changed(), ["tasks", "visibleTasks", "selectedTask"])
        XCTAssertEqual(state.selectedTask, renamed)
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }
}

/// Reports which catalog properties changed, in declaration order, as views reading each one would see them.
@MainActor
private final class CatalogProbes {
    private let probes: [(String, ObservationProbe)]

    init(_ state: TaskCatalogStore) {
        probes = [
            ("tasks", ObservationProbe { _ = state.tasks }),
            ("tab", ObservationProbe { _ = state.tab }),
            ("visibleTasks", ObservationProbe { _ = state.visibleTasks }),
            ("selectedTaskID", ObservationProbe { _ = state.selectedTaskID }),
            ("selectedTask", ObservationProbe { _ = state.selectedTask }),
        ]
    }

    func changed() -> [String] { probes.filter { $0.1.consume() }.map(\.0) }
}

/// Counts the history reloads a catalog asked for.
@MainActor
private final class HistoryReloads {
    var count = 0
}
