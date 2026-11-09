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
        XCTAssertFalse(fixture.features.connection.stale)
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

        XCTAssertEqual(fixture.features.taskCatalog.tasks, [remoteTask])
        XCTAssertEqual(fixture.features.tracking.active, remoteWorklog)
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, remoteTask.id)
        XCTAssertEqual(fixture.features.connection.settings, serverSettings)
    }

    @MainActor
    func testSourceSwitchClearsCachedHistoryEvenWhenTaskIDsMatch() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        fixture.features.worklogHistory.reload()
        let initial = try await fixture.client.next()
        initial.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "old-source", reset: false))
        try await fixture.settled()

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let connect = try await fixture.client.next()
        connect.succeed(resources)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        let replacement = try await fixture.client.next()
        XCTAssertTrue(fixture.features.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.features.worklogHistory.nextCursor)
        replacement.succeed(emptyPage)
        try await fixture.settled()
    }

    @MainActor
    func testReplacementSourceChoosesItsFirstTaskWhenPreviousIDsAlsoExist() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask, secondTask], active: nil)
        try await fixture.start(resources)
        fixture.features.taskCatalog.select(secondTask.id)
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
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, firstTask.id)
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

        XCTAssertEqual(fixture.features.connection.settings, serverSettings)
        XCTAssertEqual(fixture.features.connection.statusText, "Unavailable")
        XCTAssertEqual(fixture.features.menuSources.read().runningTaskName, "Waiting for tracker state")
        XCTAssertEqual(fixture.features.tracking.timerText, "Unavailable")
        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertFalse(fixture.canStartSelectedTask)
        XCTAssertFalse(fixture.trackingAvailability.canStop)
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
        request.tested()
        try await fixture.taskValue(testing)

        XCTAssertEqual(fixture.features.connection.settings, .local)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations, [.open(.local), .test(serverSettings)])
    }

    @MainActor
    func testFailedCandidateRetainsResourcesHistoryAndSettings() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        history.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: "older", reset: false))
        try await fixture.settled()

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertEqual(request.operation, .connect(serverSettings))
        XCTAssertTrue(fixture.features.connection.changing)
        request.fail(BridgeFailure(message: "Candidate offline"))
        let connected = try await fixture.taskValue(connecting)

        XCTAssertFalse(connected)
        XCTAssertEqual(fixture.features.connection.settings, .local)
        XCTAssertEqual(fixture.features.taskCatalog.tasks, [firstTask])
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertEqual(fixture.features.worklogHistory.nextCursor, "older")
        XCTAssertEqual(fixture.features.connection.message, "Connection unchanged. Candidate offline")
        XCTAssertFalse(fixture.features.connection.changing)
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testSuccessfulConnectionToEmptySourceClearsPreviousHistoryAndErrors() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.features.worklogHistory.reload()
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "History unavailable"))
        try await fixture.settled()
        XCTAssertTrue(fixture.features.worklogHistory.unavailable)

        fixture.trackingActions.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        stop.fail(BridgeFailure(message: "Write failed", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.succeed(TaskListResources(tasks: [firstTask], active: activeWorklog))
        try await fixture.settled()
        XCTAssertEqual(
            fixture.features.tracking.recordedError,
            "Could not confirm the change. Check the reloaded data before trying again. Write failed")

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        request.succeed(emptyTaskListResources)
        let connected = try await fixture.taskValue(connecting)

        XCTAssertTrue(connected)
        XCTAssertEqual(fixture.features.connection.settings, serverSettings)
        XCTAssertEqual(fixture.settings.writes, [serverSettings])
        XCTAssertTrue(fixture.features.taskCatalog.tasks.isEmpty)
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertNil(fixture.features.taskCatalog.selectedTaskID)
        XCTAssertTrue(fixture.features.worklogHistory.worklogs.isEmpty)
        XCTAssertNil(fixture.features.worklogHistory.nextCursor)
        XCTAssertNil(fixture.features.worklogHistory.error)
        XCTAssertNil(fixture.features.tracking.recordedError)
        XCTAssertFalse(fixture.features.worklogHistory.unavailable)
        XCTAssertEqual(fixture.features.connection.statusText, "Connected")
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

        XCTAssertEqual(fixture.features.taskCatalog.tasks, [secondTask])
        XCTAssertEqual(fixture.features.taskCatalog.selectedTaskID, secondTask.id)
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertEqual(fixture.features.menuSources.read().runningTaskName, "No timer running")
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
        XCTAssertEqual(fixture.features.connection.message, "Connection unchanged. Enter a server URL.")
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
        XCTAssertTrue(fixture.settings.writes.isEmpty)
    }

    @MainActor
    func testConnectionFailurePublishesSoTrackingAvailabilityUpdates() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertTrue(fixture.trackingAvailability.canStart(taskID: firstTask.id))

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertFalse(fixture.trackingAvailability.canStart(taskID: firstTask.id))
    }

    @MainActor
    func testStartupFailureUpdatesTheConnectionStatus() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }

        fixture.sync.start()
        let open = try await fixture.client.next()
        open.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.connection.statusText, "Unavailable")
        XCTAssertEqual(fixture.features.connection.message, "Server offline")
        XCTAssertTrue(fixture.features.connection.stale)
    }

    @MainActor
    func testFailuresAndConnectionChangesUpdateTheStores() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: nil))
        let connection = fixture.features.connection

        fixture.features.taskCatalog.select(secondTask.id)
        let history = try await fixture.client.next()
        history.fail(BridgeFailure(message: "History unavailable", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertNotNil(fixture.features.worklogHistory.error)
        XCTAssertTrue(fixture.features.worklogHistory.unavailable)

        let connecting = Task { await fixture.sync.connect(serverSettings) }
        let request = try await fixture.client.next()
        XCTAssertTrue(connection.changing)
        request.fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        _ = try await fixture.taskValue(connecting)
        try await fixture.settled()
        XCTAssertFalse(connection.changing)
        XCTAssertNotNil(connection.message)

        let connected = Task { await fixture.sync.connect(serverSettings) }
        let accepted = try await fixture.client.next()
        accepted.succeed(TaskListResources(tasks: [firstTask], active: nil))
        try await fixture.client.next().succeed(emptyPage)
        let didConnect = try await fixture.taskValue(connected)
        XCTAssertTrue(didConnect)
        try await fixture.settled()
        XCTAssertEqual(connection.settings, serverSettings)
    }

    @MainActor
    func testUnchangedPollNotifiesNoConnectionReader() async throws {
        let fixture = Fixture(saved: serverSettings)
        defer { fixture.cleanup() }
        let resources = TaskListResources(tasks: [firstTask], active: nil)
        try await fixture.start(resources)
        let probes = ConnectionProbes(fixture.features.connection)

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
        let probes = ConnectionProbes(fixture.features.connection)

        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["statusText", "message", "stale"])
        XCTAssertEqual(fixture.features.connection.statusText, "Unavailable")
        fixture.scheduler.poll?.fire()
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), [])
        XCTAssertEqual(fixture.features.connection.failures, 2)
        fixture.scheduler.poll?.fire()
        try await fixture.client.next().succeed(resources)
        try await fixture.settled()
        XCTAssertEqual(probes.changed(), ["statusText", "message", "stale"])
        XCTAssertEqual(fixture.features.connection.statusText, "Connected")
        XCTAssertNil(fixture.features.connection.message)
        XCTAssertEqual(fixture.features.connection.failures, 0)
    }

    @MainActor
    func testConnectionChangeNotifiesChangingReadersWhenItStartsAndEnds() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let probes = ConnectionProbes(fixture.features.connection)

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
        XCTAssertEqual(fixture.features.connection.settings, serverSettings)
        XCTAssertEqual(fixture.features.connection.statusText, "Connected")
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
