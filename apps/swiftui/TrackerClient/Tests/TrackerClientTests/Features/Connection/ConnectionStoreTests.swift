import XCTest
@testable import TrackerClient

final class ConnectionStoreTests: XCTestCase {
    @MainActor
    func testConnectSwitchesToTheNormalizedSettingsWhileMarkedAsChanging() async {
        let store = ConnectionStore(settings: .local)
        let calls = ConnectionSwitches()
        store.switchConnection = { [weak store] settings in
            calls.settings.append(settings)
            calls.changing.append(store?.changing ?? false)
        }

        let connected = await store.connect(ConnectionSettings(mode: .server, serverURL: " https://tracker.example\n"))

        XCTAssertTrue(connected)
        XCTAssertEqual(calls.settings, [serverSettings])
        XCTAssertEqual(calls.changing, [true])
        XCTAssertFalse(store.changing)
        XCTAssertNil(store.message)
    }

    @MainActor
    func testConnectReturnsFalseWithoutSwitchingWhileAnotherChangeRuns() async {
        let store = ConnectionStore(settings: .local)
        let calls = ConnectionSwitches()
        store.switchConnection = { [weak store] settings in
            calls.settings.append(settings)
            guard let store, calls.settings.count == 1 else { return }
            calls.nestedResults.append(await store.connect(.local))
        }

        let connected = await store.connect(serverSettings)

        XCTAssertTrue(connected)
        XCTAssertEqual(calls.settings, [serverSettings])
        XCTAssertEqual(calls.nestedResults, [false])
        XCTAssertFalse(store.changing)
    }

    @MainActor
    func testRejectedSettingsKeepTheConnectionAndShowTheReason() async {
        let store = ConnectionStore(settings: .local)
        let calls = ConnectionSwitches()
        store.switchConnection = { settings in
            calls.settings.append(settings)
            throw BridgeFailure(message: "Server unavailable")
        }

        let rejected = await store.connect(serverSettings)

        XCTAssertFalse(rejected)
        XCTAssertEqual(calls.settings, [serverSettings])
        XCTAssertEqual(store.message, "Connection unchanged. Server unavailable")
        XCTAssertEqual(store.settings, .local)
        XCTAssertFalse(store.changing)

        let invalid = await store.connect(ConnectionSettings(mode: .server, serverURL: " \t"))

        XCTAssertFalse(invalid)
        XCTAssertEqual(calls.settings, [serverSettings], "Invalid settings never reach the tracker.")
        XCTAssertEqual(store.message, "Connection unchanged. Enter a server URL.")
        XCTAssertFalse(store.changing)
    }

    @MainActor
    func testRefreshReloadsTheTaskList() async {
        let store = ConnectionStore(settings: .local)
        let calls = ConnectionSwitches()
        store.refreshTaskList = { calls.refreshes += 1 }

        store.refresh()

        XCTAssertEqual(calls.refreshes, 1)
    }

    @MainActor
    func testAcceptedTaskListConfirmsTheConnectionAndLiftsAFailure() async {
        let store = ConnectionStore(settings: serverSettings)
        store.recordFailure(BridgeFailure(message: "Tracker unavailable", kind: "unavailable"))
        store.recordFailure(BridgeFailure(message: "Incompatible server", kind: "protocol"))
        XCTAssertEqual(store.failures, 2)
        XCTAssertTrue(store.protocolBlocked)

        store.acceptTaskList()

        XCTAssertTrue(store.confirmed)
        XCTAssertFalse(store.stale)
        XCTAssertEqual(store.failures, 0)
        XCTAssertFalse(store.protocolBlocked)
        XCTAssertEqual(store.statusText, "Connected")
        XCTAssertNil(store.message)

        let local = ConnectionStore(settings: .local)
        local.acceptTaskList()
        XCTAssertEqual(local.statusText, "Local database")
    }

    @MainActor
    func testAcceptedConnectionTakesItsSettings() async {
        let store = ConnectionStore(settings: .local)

        store.acceptConnection(serverSettings)

        XCTAssertEqual(store.settings, serverSettings)
    }

    @MainActor
    func testOnlyRefreshUnavailableAndProtocolFailuresRequireRecovery() async {
        let store = ConnectionStore(settings: .local)

        XCTAssertTrue(store.requiresRecovery(after: BridgeFailure(message: "Stale", requiresRefresh: true)))
        XCTAssertTrue(store.requiresRecovery(after: BridgeFailure(message: "Offline", kind: "unavailable")))
        XCTAssertTrue(store.requiresRecovery(after: BridgeFailure(message: "Unsupported", kind: "protocol")))
        XCTAssertFalse(store.requiresRecovery(after: BridgeFailure(message: "Rejected", kind: "validation")))
        XCTAssertFalse(store.requiresRecovery(after: BridgeFailure(message: "Failed")))
        XCTAssertFalse(store.requiresRecovery(after: CancellationError()))
    }
}

private final class ConnectionSwitches {
    var settings: [ConnectionSettings] = []
    var changing: [Bool] = []
    var nestedResults: [Bool] = []
    var refreshes = 0
}
