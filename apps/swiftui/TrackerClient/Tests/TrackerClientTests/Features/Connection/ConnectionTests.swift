import Foundation
import XCTest
@testable import TrackerClient

final class ConnectionTests: XCTestCase {
    @MainActor
    func testNormalizedSettingsTrimTheServerURLAndRequireOneOnlyInServerMode() async throws {
        let server = ConnectionSettings(mode: .server, serverURL: " \nhttps://tracker.example\t")
        XCTAssertEqual(try server.normalized(), serverSettings)
        let local = ConnectionSettings(mode: .local, serverURL: " \n")
        XCTAssertEqual(try local.normalized(), ConnectionSettings(mode: .local, serverURL: ""))
        XCTAssertThrowsError(try ConnectionSettings(mode: .server, serverURL: " \t").normalized()) {
            XCTAssertEqual($0.localizedDescription, "Enter a server URL.")
        }
    }

    @MainActor
    func testSuccessfulConnectionRestartsPollingAfterAProtocolFailure() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.sync.refresh()
        let incompatible = try await fixture.client.next()
        incompatible.fail(BridgeFailure(message: "Unsupported protocol", kind: "protocol"))
        try await fixture.settled()
        XCTAssertNil(fixture.scheduler.poll)

        let connecting = Task { await fixture.sync.connect(serverSettings) }
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
        XCTAssertFalse(fixture.session.connection.stale)
    }

    @MainActor
    func testSourceSwitchPublishesTheReplacementTasksAndActiveTimer() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let remoteTask = TaskItem(id: "remote-task", name: "Remote task", archived: false, latestStart: nil)
        let remoteWorklog = WorklogItem(
            id: "remote-worklog", taskId: remoteTask.id, start: activeWorklog.start, end: nil)
        let connecting = Task { await fixture.sync.connect(serverSettings) }
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
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, remoteTask.id)
        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
    }

    @MainActor
    func testSourceSwitchClearsCachedHistoryEvenWhenTaskIDsMatch() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.sync.retryHistory()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "old-source", reset: false))
        try await fixture.settled()

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let connect = try await fixture.client.next()
        connect.succeed(resources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let replacement = try await fixture.client.next()
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
        replacement.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testReplacementSourceChoosesItsFirstTaskWhenPreviousIDsAlsoExist() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.sync.selectTask(secondTask.id)
        let previousHistory = try await fixture.client.next()
        previousHistory.succeed(emptyPage)
        try await fixture.settled()

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(resources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let replacementHistory = try await fixture.client.next()
        XCTAssertEqual(replacementHistory.operation, .history(task: firstTask.id, cursor: nil))
        replacementHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, firstTask.id)
    }

    @MainActor
    func testOfflineStartupKeepsSavedServerAndDisablesCommands() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        fixture.sync.start()
        let open = try await fixture.client.next()
        XCTAssertEqual(open.operation, .open(serverSettings))
        open.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()

        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
        XCTAssertEqual(fixture.session.connection.statusText, "Unavailable")
        XCTAssertEqual(fixture.session.runningTaskName, "Waiting for tracker state")
        XCTAssertEqual(fixture.session.tracking.timerText, "Unavailable")
        XCTAssertTrue(fixture.session.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertFalse(fixture.trackingActions.canStopTracking)
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
        let testing = Task { try await testConnectionSettings(candidate, client: fixture.client) }
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .test(serverSettings))
        XCTAssertTrue(fixture.client.hasPendingRequests)
        request.tested()
        try await fixture.taskValue(testing)

        XCTAssertEqual(fixture.session.connection.settings, .local)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations, [.open(.local), .test(serverSettings)])
    }

    @MainActor
    func testFailedCandidateRetainsResourcesHistoryAndSettings() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .connect(serverSettings))
        XCTAssertTrue(fixture.session.connection.changing)
        request.fail(BridgeFailure(message: "Candidate offline"))
        let connected = try await fixture.taskValue(connecting)

        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connection.settings, .local)
        XCTAssertEqual(fixture.session.tasks, [firstTask])
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertEqual(fixture.session.worklogHistory.worklogs, [oldWorklog])
        XCTAssertEqual(fixture.session.worklogHistory.nextCursor, "older")
        XCTAssertEqual(fixture.session.connection.message, "Connection unchanged. Candidate offline")
        XCTAssertFalse(fixture.session.connection.changing)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testSuccessfulConnectionToEmptySourceClearsPreviousHistoryAndErrors() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.sync.retryHistory()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.session.worklogHistory.unavailable)

        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        stop.fail(BridgeFailure(message: "Write failed", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(
            fixture.session.tracking.recordedError,
            "Could not confirm the change. Check the reloaded data before trying again. Write failed")

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)

        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
        XCTAssertEqual(fixture.settings.writes, [serverSettings])
        XCTAssertTrue(fixture.session.tasks.isEmpty)
        XCTAssertNil(fixture.session.active)
        XCTAssertNil(fixture.session.taskCatalog.selectedTaskID)
        XCTAssertTrue(fixture.session.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.session.worklogHistory.nextCursor)
        XCTAssertNil(fixture.session.worklogHistory.error)
        XCTAssertNil(fixture.session.tracking.recordedError)
        XCTAssertFalse(fixture.session.worklogHistory.unavailable)
        XCTAssertEqual(fixture.session.connection.statusText, "Connected")
    }

    @MainActor
    func testSuccessfulSourceSwitchReplacesTasksAndLoadsNewSelection() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, archivedTask], active: activeWorklog))
        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(TaskListResources(tasks: [secondTask], active: nil))
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()

        XCTAssertEqual(fixture.session.tasks, [secondTask])
        XCTAssertEqual(fixture.session.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.session.active)
        XCTAssertEqual(fixture.session.runningTaskName, "No timer running")
    }

    @MainActor
    func testEmptyServerURLFailsBeforeClientCall() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        let connecting = Task {
            await fixture.sync.connect(ConnectionSettings(mode: .server, serverURL: " \n"))
        }
        let connected = try await fixture.taskValue(connecting)
        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.session.connection.message, "Connection unchanged. Enter a server URL.")
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testConnectionFailurePublishesSoTrackingAvailabilityUpdates() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let harness = PresentationHarness(session: fixture.session)
        XCTAssertTrue(harness.presentation.canStartTracking(taskID: firstTask.id))

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await harness.settled(fixture)
        XCTAssertFalse(harness.presentation.canStartTracking(taskID: firstTask.id))
    }

    @MainActor
    func testUnchangedPollNotifiesNoConnectionReader() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = ConnectionProbes(fixture.session.connection)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), [])
    }

    @MainActor
    func testFailureAndRecoveryNotifyOnlyTheReadersOfChangedValues() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = ConnectionProbes(fixture.session.connection)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["statusText", "message", "stale"])
        XCTAssertEqual(fixture.session.connection.statusText, "Unavailable")
        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), [])
        XCTAssertEqual(fixture.session.connection.failures, 2)
        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["statusText", "message", "stale"])
        XCTAssertEqual(fixture.session.connection.statusText, "Connected")
        XCTAssertNil(fixture.session.connection.message)
        XCTAssertEqual(fixture.session.connection.failures, 0)
    }

    @MainActor
    func testConnectionChangeNotifiesChangingReadersWhenItStartsAndEnds() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let probes = ConnectionProbes(fixture.session.connection)

        let rejected = Task { await fixture.sync.connect(serverSettings) }
        let candidate = try await fixture.client.next()
        XCTAssertEqual(probes.changed(), ["changing"])
        candidate.fail(BridgeFailure(message: "Candidate offline"))
        _ = try await fixture.taskValue(rejected)
        XCTAssertEqual(probes.changed(), ["message", "changing"])

        let accepted = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertEqual(probes.changed(), ["changing"])
        request.succeed(TaskListResources(tasks: [firstTask], active: nil))
        try await fixture.client.next().succeed(emptyPage)
        let didConnect = try await fixture.taskValue(accepted)
        XCTAssertTrue(didConnect)
        XCTAssertEqual(probes.changed(), ["settings", "statusText", "message", "changing"])
        XCTAssertEqual(fixture.session.connection.settings, serverSettings)
        XCTAssertEqual(fixture.session.connection.statusText, "Connected")
    }
}

/// Reports which connection properties changed, in declaration order, as views reading each one would see them.
@MainActor
private final class ConnectionProbes {
    private let probes: [(String, ObservationProbe)]

    init(_ store: ConnectionStore) {
        probes = [
            ("settings", ObservationProbe { _ = store.settings }),
            ("statusText", ObservationProbe { _ = store.statusText }),
            ("message", ObservationProbe { _ = store.message }),
            ("changing", ObservationProbe { _ = store.changing }),
            ("stale", ObservationProbe { _ = store.stale }),
            ("confirmed", ObservationProbe { _ = store.confirmed }),
        ]
    }

    func changed() -> [String] { probes.filter { $0.1.consume() }.map(\.0) }
}
