import XCTest
@testable import TrackerClient

final class TaskCatalogTests: XCTestCase {
    @MainActor
    func testRememberedRestoredTaskIgnoresMissingAndArchivedTasks() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.apply([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        XCTAssertTrue(state.changeTab(.archived))
        state.rememberRestoredTask(secondTask.id)
        state.rememberRestoredTask("missing-task")
        state.rememberRestoredTask(archivedTask.id)

        XCTAssertTrue(state.changeTab(.active))
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
    }

    @MainActor
    func testChangingSelectedWorklogStartReloadsHistoryWithoutChangingItsID() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: activeWorklog))
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: firstTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)

        XCTAssertTrue(state.apply(tasks, previousActive: activeWorklog, active: corrected))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testReplacingSelectedWorklogReloadsHistoryEvenWhenStartIsUnchanged() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: activeWorklog))
        let replacement = WorklogItem(
            id: "replacement-worklog", taskId: firstTask.id,
            start: activeWorklog.start, end: nil)

        XCTAssertTrue(state.apply(tasks, previousActive: activeWorklog, active: replacement))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testSelectedHistoryReloadsWhenTrackingStartsStopsOrMovesToAnotherTask() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        let other = WorklogItem(
            id: "other-worklog", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)

        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: activeWorklog))
        XCTAssertTrue(state.apply(tasks, previousActive: activeWorklog, active: nil))
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: activeWorklog))
        XCTAssertTrue(state.apply(tasks, previousActive: activeWorklog, active: other))
        XCTAssertTrue(state.apply(tasks, previousActive: other, active: activeWorklog))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testUnrelatedTrackingChangesDoNotReloadSelectedHistory() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        let previous = WorklogItem(
            id: "other-worklog", taskId: secondTask.id,
            start: activeWorklog.start, end: nil)
        let replacement = WorklogItem(
            id: "new-other-worklog", taskId: secondTask.id,
            start: "2025-01-01T00:00:00.000Z", end: nil)

        XCTAssertFalse(state.apply(tasks, previousActive: nil, active: previous))
        XCTAssertFalse(state.apply(tasks, previousActive: previous, active: replacement))
        XCTAssertFalse(state.apply(tasks, previousActive: replacement, active: nil))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testStableCatalogAndWorklogDoNotReloadHistory() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: activeWorklog))

        XCTAssertFalse(state.apply(tasks, previousActive: activeWorklog, active: activeWorklog))
        XCTAssertEqual(state.selectedTask, firstTask)
        XCTAssertEqual(state.visibleTasks, tasks)
    }

    @MainActor
    func testLatestStartReloadsOnlyTheSelectedTaskHistory() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.apply([firstTask, secondTask], previousActive: nil, active: nil))
        let updatedOther = TaskItem(
            id: secondTask.id, name: secondTask.name, archived: false,
            latestStart: "2025-01-01T00:00:00.000Z")
        let updatedSelected = TaskItem(
            id: firstTask.id, name: firstTask.name, archived: false,
            latestStart: "2025-01-01T01:00:00.000Z")

        XCTAssertFalse(state.apply([firstTask, updatedOther], previousActive: nil, active: nil))
        XCTAssertTrue(state.apply([updatedSelected, updatedOther], previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTask, updatedSelected)
        XCTAssertTrue(state.apply([firstTask, updatedOther], previousActive: nil, active: nil))
    }

    @MainActor
    func testNilUnknownHiddenAndRepeatedSelectionsLeaveCatalogUnchanged() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.apply([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))

        XCTAssertFalse(state.select(nil))
        XCTAssertFalse(state.select("missing-task"))
        XCTAssertFalse(state.select(archivedTask.id))
        XCTAssertFalse(state.select(firstTask.id))
        XCTAssertFalse(state.changeTab(.active))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
        XCTAssertTrue(state.select(secondTask.id))
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
        XCTAssertTrue(state.changeTab(.archived))
        XCTAssertFalse(state.select(firstTask.id))
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
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.select(secondTask.id))
        XCTAssertTrue(state.changeTab(.archived))
        XCTAssertTrue(state.select(otherArchived.id))
        XCTAssertTrue(state.changeTab(.active))
        XCTAssertEqual(state.selectedTaskID, secondTask.id)
        XCTAssertTrue(state.changeTab(.archived))
        XCTAssertEqual(state.selectedTaskID, otherArchived.id)
        XCTAssertTrue(state.changeTab(.active))

        XCTAssertTrue(state.apply([firstTask, archivedTask], previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
        XCTAssertTrue(state.changeTab(.archived))
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)
        XCTAssertTrue(state.apply([firstTask], previousActive: nil, active: nil))
        XCTAssertNil(state.selectedTaskID)
        XCTAssertNil(state.selectedTask)
        XCTAssertTrue(state.visibleTasks.isEmpty)
    }

    @MainActor
    func testResetSelectionsClearsBothTabMemoriesBeforeLoadingAnotherSource() async {
        let state = TaskCatalogStore()
        let otherArchived = TaskItem(
            id: "other-archived-task", name: "Other archived task",
            archived: true, latestStart: nil)
        let tasks = [firstTask, secondTask, archivedTask, otherArchived]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.select(secondTask.id))
        XCTAssertTrue(state.changeTab(.archived))
        XCTAssertTrue(state.select(otherArchived.id))

        state.resetSelections()
        XCTAssertNil(state.selectedTaskID)
        XCTAssertEqual(state.tab, .archived)
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTaskID, archivedTask.id)
        XCTAssertTrue(state.changeTab(.active))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testRestoredTaskDoesNotReplaceTheFallbackSelectionRememberedForItsTab() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask, archivedTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.select(secondTask.id))
        XCTAssertTrue(state.apply([firstTask, archivedTask], previousActive: nil, active: nil))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
        XCTAssertTrue(state.changeTab(.archived))

        XCTAssertFalse(state.apply(tasks, previousActive: nil, active: nil))
        XCTAssertTrue(state.changeTab(.active))
        XCTAssertEqual(state.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testUnchangedCatalogAndRepeatedSelectionsNotifyNoReader() async {
        let state = TaskCatalogStore()
        let tasks = [firstTask, secondTask, archivedTask]
        XCTAssertTrue(state.apply(tasks, previousActive: nil, active: nil))
        let probes = CatalogProbes(state)

        XCTAssertFalse(state.apply(tasks, previousActive: nil, active: nil))
        XCTAssertFalse(state.select(firstTask.id))
        XCTAssertFalse(state.changeTab(.active))
        state.rememberRestoredTask(secondTask.id)
        XCTAssertEqual(probes.changed(), [])
    }

    @MainActor
    func testAnotherTaskChangeNotifiesOnlyTheReadersOfItsList() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.apply([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        let probes = CatalogProbes(state)
        let renamedArchived = TaskItem(
            id: archivedTask.id, name: "Renamed archived task", archived: true, latestStart: nil)
        let updatedOther = TaskItem(
            id: secondTask.id, name: secondTask.name, archived: false,
            latestStart: "2025-01-01T00:00:00.000Z")

        XCTAssertFalse(state.apply([firstTask, secondTask, renamedArchived], previousActive: nil, active: nil))
        XCTAssertEqual(probes.changed(), ["tasks"])
        XCTAssertFalse(state.apply([firstTask, updatedOther, renamedArchived], previousActive: nil, active: nil))
        XCTAssertEqual(probes.changed(), ["tasks", "visibleTasks"])
        XCTAssertEqual(state.visibleTasks, [firstTask, updatedOther])
    }

    @MainActor
    func testSelectionAndTabChangesNotifyTheirReaders() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.apply([firstTask, secondTask, archivedTask], previousActive: nil, active: nil))
        let probes = CatalogProbes(state)

        XCTAssertTrue(state.select(secondTask.id))
        XCTAssertEqual(probes.changed(), ["selectedTaskID", "selectedTask"])
        XCTAssertEqual(state.selectedTask, secondTask)
        XCTAssertTrue(state.changeTab(.archived))
        XCTAssertEqual(probes.changed(), ["tab", "visibleTasks", "selectedTaskID", "selectedTask"])
        XCTAssertEqual(state.visibleTasks, [archivedTask])
        XCTAssertEqual(state.selectedTask, archivedTask)
        state.resetSelections()
        XCTAssertEqual(probes.changed(), ["selectedTaskID", "selectedTask"])
        XCTAssertNil(state.selectedTask)
        XCTAssertEqual(state.tab, .archived)
    }

    @MainActor
    func testSelectedTaskChangeNotifiesItsReadersWithoutChangingTheSelection() async {
        let state = TaskCatalogStore()
        XCTAssertTrue(state.apply([firstTask, secondTask], previousActive: nil, active: nil))
        let probes = CatalogProbes(state)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)

        XCTAssertFalse(state.apply([renamed, secondTask], previousActive: nil, active: nil))
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
