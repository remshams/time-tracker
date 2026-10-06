import XCTest
@testable import TrackerClient

@MainActor
final class MemoryLastTrackedTasks: LastTrackedTaskRepository {
    var tasks: [String: String] = [:]
    var writes: [(String, String)] = []

    func load(for settings: ConnectionSettings) -> String? { tasks[settings.trackingIdentityKey] }
    func save(taskID: String, for settings: ConnectionSettings) {
        tasks[settings.trackingIdentityKey] = taskID
        writes.append((settings.trackingIdentityKey, taskID))
    }
}

final class StatusItemTests: XCTestCase {
    func testColorsUseStableTaskIDsAndSharedIndicators() {
        XCTAssertEqual(TaskColor.forTaskID("task-one"), .green)
        XCTAssertEqual(TaskColor.forTaskID("task-two"), .blue)
        XCTAssertEqual(TaskColor.forTaskID(""), .green)
        XCTAssertEqual(TaskColor.forTaskID("task-one"), TaskIndicator(taskID: "task-one", isRunning: false).color)
        XCTAssertEqual(TaskIndicator(taskID: "task-one", isRunning: true).symbol, "circle.fill")
        XCTAssertEqual(TaskIndicator(taskID: "task-one", isRunning: false).symbol, "circle")
        XCTAssertEqual(TaskIndicator(taskID: "task-one", isRunning: true, isStale: true).symbol, "circle")
        XCTAssertNil(TaskIndicator(taskID: nil, isRunning: false).color)
    }

    func testIdentityKeySeparatesLocalAndServersAndTrimsWhitespace() {
        XCTAssertEqual(ConnectionSettings.local.trackingIdentityKey, "local")
        let padded = ConnectionSettings(mode: .server, serverURL: "  http://example.test:8765/ \n")
        XCTAssertEqual(padded.trackingIdentityKey, "server:http://example.test:8765/")
        XCTAssertNotEqual(padded.trackingIdentityKey, serverSettings.trackingIdentityKey)
    }

    @MainActor
    func testStateRemembersOnlyConfirmedActiveAndDoesNotRewriteUnchangedIdentity() {
        let repository = MemoryLastTrackedTasks()
        let state = LastTrackedTaskState(repository: repository, settings: .local)
        XCTAssertNil(state.taskID)
        state.observe(TrackerSnapshot(tasks: [firstTask], active: activeWorklog), settings: .local)
        XCTAssertEqual(state.taskID, firstTask.id)
        XCTAssertEqual(repository.writes.count, 1)
        state.observe(TrackerSnapshot(tasks: [firstTask], active: nil), settings: .local)
        XCTAssertEqual(state.taskID, firstTask.id)
        XCTAssertEqual(repository.writes.count, 1)
    }

    @MainActor
    func testIdleStartupInfersLatestValidStartAndBreaksDateTiesByID() {
        let repository = MemoryLastTrackedTasks()
        let state = LastTrackedTaskState(repository: repository, settings: .local)
        let earlier = TaskItem(id: "earlier", name: "Earlier", archived: false, latestStart: "2024-12-30T12:00:00.000Z")
        let latest = TaskItem(id: "latest", name: "Latest", archived: true, latestStart: "2024-12-31T12:00:00.000Z")
        let tied = TaskItem(id: "aaa", name: "Tied", archived: false, latestStart: latest.latestStart)
        let invalid = TaskItem(id: "invalid", name: "Invalid", archived: false, latestStart: "invalid")
        state.observe(TrackerSnapshot(tasks: [earlier, latest, tied, invalid, firstTask], active: nil), settings: .local)
        XCTAssertEqual(state.taskID, tied.id)
        XCTAssertEqual(repository.writes.last?.1, tied.id)
    }

    @MainActor
    func testNewerCompletedServerTrackingUpdatesRememberedIdentityBetweenPolls() {
        let repository = MemoryLastTrackedTasks()
        repository.save(taskID: firstTask.id, for: serverSettings)
        let state = LastTrackedTaskState(repository: repository, settings: serverSettings)
        let previous = TaskItem(id: firstTask.id, name: firstTask.name, archived: false,
                                latestStart: "2024-12-30T12:00:00.000Z")
        let newer = TaskItem(id: secondTask.id, name: secondTask.name, archived: false,
                             latestStart: "2024-12-31T12:00:00.000Z")
        state.observe(TrackerSnapshot(tasks: [previous, newer], active: nil), settings: serverSettings)
        XCTAssertEqual(state.taskID, secondTask.id)
        XCTAssertEqual(repository.load(for: serverSettings), secondTask.id)
        let tied = TaskItem(id: "aaa", name: "Tied", archived: false, latestStart: newer.latestStart)
        state.observe(TrackerSnapshot(tasks: [previous, newer, tied], active: nil), settings: serverSettings)
        XCTAssertEqual(state.taskID, secondTask.id, "Equal timestamps must not replace a known last task.")
        state.observe(TrackerSnapshot(tasks: [previous, tied], active: nil), settings: serverSettings)
        XCTAssertEqual(state.taskID, secondTask.id, "Deleted last tasks keep their identity for the menu fallback.")
    }

    @MainActor
    func testConfirmedDatedTaskSupersedesRememberedTaskWithoutDate() {
        let state = LastTrackedTaskState(repository: nil, settings: .local)
        state.observe(TrackerSnapshot(tasks: [firstTask], active: activeWorklog), settings: .local)
        let newer = TaskItem(id: secondTask.id, name: secondTask.name, archived: false,
                             latestStart: "2024-12-31T12:00:00.000Z")
        state.observe(TrackerSnapshot(tasks: [firstTask, newer], active: nil), settings: .local)
        XCTAssertEqual(state.taskID, secondTask.id)
    }

    @MainActor
    func testConnectionIsolationWorksWithoutRepositoryAndAcrossRestarts() {
        let repository = MemoryLastTrackedTasks()
        for persistence in [nil, repository] as [(any LastTrackedTaskRepository)?] {
            let state = LastTrackedTaskState(repository: persistence, settings: .local)
            state.observe(TrackerSnapshot(tasks: [firstTask], active: activeWorklog), settings: .local)
            state.observe(emptySnapshot, settings: serverSettings)
            XCTAssertNil(state.taskID)
            let remoteActive = WorklogItem(id: "remote", taskId: secondTask.id, start: activeWorklog.start, end: nil)
            state.observe(TrackerSnapshot(tasks: [secondTask], active: remoteActive), settings: serverSettings)
            state.observe(TrackerSnapshot(tasks: [firstTask], active: nil), settings: .local)
            XCTAssertEqual(state.taskID, firstTask.id)
        }
        XCTAssertEqual(LastTrackedTaskState(repository: repository, settings: .local).taskID, firstTask.id)
        XCTAssertEqual(LastTrackedTaskState(repository: repository, settings: serverSettings).taskID, secondTask.id)
    }

    @MainActor
    func testLeftClickStopsThenRestartsLastTaskAndDoesNotDoubleSubmit() async throws {
        let repository = MemoryLastTrackedTasks()
        let fixture = Fixture(lastTrackedTasks: repository)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        XCTAssertEqual(fixture.session.menuPrimaryAction, .stop(worklogID: activeWorklog.id))
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .stop(worklogID: activeWorklog.id))
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: "2025-01-01T00:00:00.000Z"))
        stop.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        let stoppedHistory = try await fixture.client.next()
        stoppedHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.select(secondTask.id)
        let selectionHistory = try await fixture.client.next()
        selectionHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .start(taskID: firstTask.id))
        let start = try await fixture.client.next()
        XCTAssertEqual(start.operation, .start(task: firstTask.id, expected: nil, at: "2025-01-01T00:00:00.000Z"))
        start.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(repository.writes.count, 1)
    }

    @MainActor
    func testNoPreviousDeletedAndArchivedTasksOpenMenuWithoutWrites() async throws {
        for remembered in [nil, "deleted-task", archivedTask.id] {
            let repository = MemoryLastTrackedTasks()
            repository.tasks[ConnectionSettings.local.trackingIdentityKey] = remembered
            let fixture = Fixture(lastTrackedTasks: repository)
            defer { fixture.cleanup() }
            try await fixture.start(TrackerSnapshot(tasks: [firstTask, archivedTask], active: nil))
            let count = fixture.client.operations.count
            XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .openMenu)
            XCTAssertEqual(fixture.client.operations.count, count)
            let label = TrackerMenuLabelContent(fixture.session, showDailyTotal: false)
            XCTAssertEqual(label.indicator.taskID, remembered)
            XCTAssertEqual(label.symbol, "circle")
            if remembered == nil { XCTAssertEqual(label.help, "No task tracked yet") }
            if remembered == "deleted-task" {
                XCTAssertEqual(label.help, "Stopped. Last tracked task is no longer available")
            }
            if remembered == archivedTask.id { XCTAssertEqual(label.help, "Stopped. Last tracked: Archived task") }
        }
    }

    @MainActor
    func testRestartLoadsLastIdentityAndResolvesRenamedTaskFromConfirmedSnapshot() async throws {
        let repository = MemoryLastTrackedTasks()
        repository.save(taskID: firstTask.id, for: .local)
        let fixture = Fixture(lastTrackedTasks: repository)
        defer { fixture.cleanup() }
        XCTAssertEqual(fixture.session.lastTrackedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        let renamed = TaskItem(id: firstTask.id, name: "Renamed task", archived: false, latestStart: nil)
        try await fixture.start(TrackerSnapshot(tasks: [renamed], active: nil))
        let label = TrackerMenuLabelContent(fixture.session, showDailyTotal: false)
        XCTAssertEqual(label.help, "Stopped. Last tracked: Renamed task")
        XCTAssertEqual(label.taskColor, TaskColor.forTaskID(firstTask.id))
        XCTAssertEqual(fixture.session.menuPrimaryAction, .start(taskID: firstTask.id))
    }

    @MainActor
    func testBusyOfflineSleepLockAndShutdownCannotWriteThroughStatusItem() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        fixture.session.sleep()
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        fixture.session.startTracking(taskID: firstTask.id)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        fixture.session.wake()
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        let refresh = try await fixture.client.next()
        refresh.fail(BridgeFailure(message: "Offline"))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        let label = TrackerMenuLabelContent(fixture.session, showDailyTotal: false)
        XCTAssertEqual(label.symbol, "circle")
        XCTAssertEqual(label.help, "Tracking status unavailable. Last confirmed task: First task")
        XCTAssertEqual(fixture.session.lastTrackedTaskID, firstTask.id)
        fixture.session.shutdown()
        let count = fixture.client.operations.count
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        XCTAssertEqual(fixture.client.operations.count, count)
    }

    @MainActor
    func testFailedConnectionKeepsIdentityAndSuccessfulSwitchLoadsOnlyItsTask() async throws {
        let repository = MemoryLastTrackedTasks()
        repository.save(taskID: secondTask.id, for: serverSettings)
        let fixture = Fixture(lastTrackedTasks: repository)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        let failedSwitch = Task { await fixture.session.connect(serverSettings) }
        let failedRequest = try await fixture.client.next()
        failedRequest.fail(BridgeFailure(message: "Unavailable server"))
        let failed = try await fixture.taskValue(failedSwitch)
        XCTAssertFalse(failed)
        XCTAssertEqual(fixture.session.lastTrackedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        let switchServer = Task { await fixture.session.connect(serverSettings) }
        let connect = try await fixture.client.next()
        connect.succeed(TrackerSnapshot(tasks: [secondTask], active: nil))
        let remoteHistory = try await fixture.client.next()
        remoteHistory.succeed(emptyPage)
        let connected = try await fixture.taskValue(switchServer)
        XCTAssertTrue(connected)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.lastTrackedTaskID, secondTask.id)
        XCTAssertEqual(fixture.session.menuPrimaryAction, .start(taskID: secondTask.id))
        XCTAssertEqual(repository.tasks[ConnectionSettings.local.trackingIdentityKey], firstTask.id)
        XCTAssertEqual(repository.tasks[serverSettings.trackingIdentityKey], secondTask.id)
    }

    @MainActor
    func testFailedStartDoesNotRememberAttemptedTaskAndReconcilesBeforeClicks() async throws {
        let repository = MemoryLastTrackedTasks()
        repository.save(taskID: firstTask.id, for: .local)
        let fixture = Fixture(lastTrackedTasks: repository)
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        fixture.session.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        start.fail(BridgeFailure(message: "Rejected start"))
        let reconciliation = try await fixture.client.next()
        XCTAssertEqual(reconciliation.operation, .snapshot)
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        XCTAssertEqual(fixture.session.lastTrackedTaskID, firstTask.id)
        reconciliation.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: nil))
        try await fixture.settled()
        XCTAssertEqual(fixture.session.lastTrackedTaskID, firstTask.id)
        XCTAssertEqual(fixture.session.menuPrimaryAction, .start(taskID: firstTask.id))
        XCTAssertEqual(repository.writes.count, 1)
    }

    @MainActor
    func testOpenMenuFreezesDotButLiveClickUsesLatestConfirmedWorklog() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask, secondTask], active: activeWorklog))
        let observer = TrackerMenuPresentationObserver(session: fixture.session, showDailyTotal: false)
        fixture.session.onChange = { observer.update(from: fixture.session, showDailyTotal: false) }
        observer.menuOpened(from: fixture.session, showDailyTotal: false)
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        let replacement = WorklogItem(id: "replacement", taskId: secondTask.id, start: activeWorklog.start, end: nil)
        refresh.succeed(TrackerSnapshot(tasks: [firstTask, secondTask], active: replacement))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(observer.label.indicator.taskID, firstTask.id)
        XCTAssertEqual(fixture.session.menuPrimaryAction, .stop(worklogID: replacement.id))
        observer.menuClosed(from: fixture.session, showDailyTotal: false)
        XCTAssertEqual(observer.label.indicator.taskID, secondTask.id)
        XCTAssertEqual(observer.label.status, "Tracking: Second task")
    }

    @MainActor
    func testScreenLockBlocksStatusClickWithoutOverridingAutomation() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TrackerSnapshot(tasks: [firstTask], active: activeWorklog))
        fixture.session.screenLocked(at: fixture.clock.now)
        let count = fixture.client.operations.count
        XCTAssertEqual(fixture.session.performMenuPrimaryAction(), .disabled)
        XCTAssertEqual(fixture.client.operations.count, count)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertEqual(fixture.session.menuPrimaryAction, .stop(worklogID: activeWorklog.id))
    }
}
