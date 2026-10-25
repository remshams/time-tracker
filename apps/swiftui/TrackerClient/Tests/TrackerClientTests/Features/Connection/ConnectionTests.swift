import Foundation
import XCTest
@testable import TrackerClient

final class ConnectionTests: XCTestCase {
    @MainActor
    func testSuccessfulConnectionRestartsPollingAfterAProtocolFailure() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.refresh()
        let incompatible = try await fixture.client.next()
        incompatible.fail(BridgeFailure(message: "Unsupported protocol", kind: "protocol"))
        try await fixture.settled()
        XCTAssertNil(fixture.scheduler.poll)

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connection = try await fixture.client.next()
        connection.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)
        try await fixture.settled()
        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
        fixture.scheduler.poll?.fire()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(serverSettings))
        refresh.succeed(emptyTaskListResources)
        try await fixture.settled()
        XCTAssertFalse(fixture.session.isStale)
    }

    @MainActor
    func testSourceSwitchPublishesTheReplacementTasksAndActiveTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let remoteTask = TaskItem(id: "remote-task", name: "Remote task", archived: false, latestStart: nil)
        let remoteWorklog = WorklogItem(
            id: "remote-worklog", taskId: remoteTask.id, start: activeWorklog.start, end: nil)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(TaskListResources(tasks: [remoteTask], active: remoteWorklog))
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: remoteTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()

        XCTAssertEqual(fixture.session.tasks, [remoteTask])
        XCTAssertEqual(fixture.session.active, remoteWorklog)
        XCTAssertEqual(fixture.session.selectedTaskID, remoteTask.id)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
    }

    @MainActor
    func testSourceSwitchClearsCachedHistoryEvenWhenTaskIDsMatch() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.session.retryHistory()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "old-source", reset: false))
        try await fixture.settled()

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connect = try await fixture.client.next()
        connect.succeed(resources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let replacement = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogs.isEmpty)
        XCTAssertNil(fixture.session.nextCursor)
        replacement.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testReplacementSourceChoosesItsFirstTaskWhenPreviousIDsAlsoExist() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.session.select(secondTask.id)
        let previousHistory = try await fixture.client.next()
        previousHistory.succeed(emptyPage)
        try await fixture.settled()

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(resources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let replacementHistory = try await fixture.client.next()
        XCTAssertEqual(replacementHistory.operation, .history(task: firstTask.id, cursor: nil))
        replacementHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testOfflineStartupKeepsSavedServerAndDisablesCommands() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        fixture.session.start()
        let open = try await fixture.client.next()
        XCTAssertEqual(open.operation, .open(serverSettings))
        open.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertEqual(fixture.session.connectionStatusText, "Unavailable")
        XCTAssertEqual(fixture.session.runningTaskName, "Waiting for tracker state")
        XCTAssertEqual(fixture.session.timerDisplayText, "Unavailable")
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
        XCTAssertFalse(fixture.session.canStopTracking)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        XCTAssertEqual(fixture.client.operations, [.open(serverSettings)])
        XCTAssertEqual(fixture.scheduler.poll?.delay, 60)
    }

    @MainActor
    func testTestingCandidateNormalizesURLWithoutChangingSavedConnection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let candidate = ConnectionSettings(mode: .server, serverURL: " \nhttps://tracker.example\t")
        let testing = Task { try await fixture.session.testConnection(candidate) }
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .test(serverSettings))
        XCTAssertTrue(fixture.client.hasPendingRequests)
        request.tested()
        try await fixture.taskValue(testing)

        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations, [.open(.local), .test(serverSettings)])
    }

    @MainActor
    func testFailedCandidateRetainsResourcesHistoryAndSettings() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .connect(serverSettings))
        XCTAssertTrue(fixture.session.isChangingConnection)
        request.fail(BridgeFailure(message: "Candidate offline"))
        let connected = try await fixture.taskValue(connecting)

        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionSettings, .local)
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.worklogs, [oldWorklog])
        XCTAssertEqual(fixture.session.nextCursor, "older")
        XCTAssertEqual(fixture.session.connectionMessage, "Connection unchanged. Candidate offline")
        XCTAssertFalse(fixture.session.isChangingConnection)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testSuccessfulConnectionToEmptySourceClearsPreviousHistoryAndErrors() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.session.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.historyUnavailable)

        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        stop.fail(BridgeFailure(message: "Write failed", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(
            fixture.session.trackingError,
            "Could not confirm the change. Check the reloaded data before trying again. Write failed")

        let connecting = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)

        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connectionSettings, serverSettings)
        XCTAssertEqual(fixture.settings.writes, [serverSettings])
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertNil(fixture.session.active)
        XCTAssertNil(fixture.session.selectedTaskID)
        XCTAssertTrue(fixture.session.worklogs.isEmpty)
        XCTAssertNil(fixture.session.nextCursor)
        XCTAssertNil(fixture.session.error)
        XCTAssertNil(fixture.session.trackingError)
        XCTAssertFalse(fixture.session.historyUnavailable)
        XCTAssertEqual(fixture.session.connectionStatusText, "Connected")
    }

    @MainActor
    func testSuccessfulSourceSwitchReplacesTasksAndLoadsNewSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask], active: activeWorklog))
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(TaskListResources(tasks: [secondTask], active: nil))
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()

        XCTAssertEqual(fixture.session.tasks, [secondTask])
        XCTAssertEqual(fixture.session.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.session.active)
        XCTAssertEqual(fixture.session.runningTaskName, "No timer running")
    }

    @MainActor
    func testEmptyServerURLFailsBeforeClientCall() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let connecting = Task {
            await fixture.session.connect(ConnectionSettings(mode: .server, serverURL: " \n"))
        }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connectionMessage, "Connection unchanged. Enter a server URL.")
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }
}
