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
}

private final class ConnectionSwitches {
    var settings: [ConnectionSettings] = []
    var changing: [Bool] = []
    var nestedResults: [Bool] = []
    var refreshes = 0
}
