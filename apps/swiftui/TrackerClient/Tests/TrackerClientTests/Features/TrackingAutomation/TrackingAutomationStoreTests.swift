import XCTest
@testable import TrackerClient

final class TrackingAutomationStoreTests: XCTestCase {
    @MainActor
    func testEnabledNotifiesItsReadersOnlyWhenItChanges() async {
        let store = TrackingAutomationStore(repository: MemoryTrackingPreferences())
        let enabled = ObservationProbe { _ = store.enabled }
        let status = ObservationProbe { _ = store.statusText }

        store.setPauseOnScreenLock(false)
        XCTAssertFalse(enabled.consume())
        store.setPauseOnScreenLock(true)
        XCTAssertTrue(store.enabled)
        XCTAssertTrue(enabled.consume())
        XCTAssertFalse(status.consume())
    }

    @MainActor
    func testSettingThePreferenceSavesItAndReportsOnlyChanges() async {
        let repository = MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true))
        let store = TrackingAutomationStore(repository: repository)
        let changes = PreferenceChanges()
        store.publishChanges = { changes.count += 1 }
        XCTAssertTrue(store.enabled)

        store.setPauseOnScreenLock(true)
        XCTAssertEqual(changes.count, 0)
        XCTAssertEqual(repository.writes, [])
        store.setPauseOnScreenLock(false)
        XCTAssertFalse(store.enabled)
        XCTAssertEqual(changes.count, 1)
        XCTAssertEqual(repository.writes, [TrackingPreferences()])
        store.setPauseOnScreenLock(true)
        XCTAssertEqual(changes.count, 2)
        XCTAssertEqual(repository.writes, [TrackingPreferences(), TrackingPreferences(pauseOnScreenLock: true)])
    }

    @MainActor
    func testTurningThePreferenceOffDropsAPendingResume() async {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        XCTAssertNotNil(store.pausedStatusText)

        store.setPauseOnScreenLock(false)
        XCTAssertNil(store.pausedStatusText)
    }

    @MainActor
    func testStatusTextChangesOnlyWhenPublishedAndNotifiesOnlyForNewText() async {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        let status = ObservationProbe { _ = store.statusText }
        let enabled = ObservationProbe { _ = store.enabled }
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        XCTAssertNil(store.statusText, "The status waits for the next publish.")
        XCTAssertFalse(status.consume())

        store.publish()
        XCTAssertEqual(store.statusText, "Paused while screen is locked")
        XCTAssertTrue(status.consume())
        store.publish()
        XCTAssertFalse(status.consume())

        XCTAssertEqual(store.unlock(at: Date(timeIntervalSince1970: 90))?.taskID, firstTask.id)
        store.publish()
        XCTAssertNil(store.statusText)
        XCTAssertTrue(status.consume())
        XCTAssertFalse(enabled.consume())
    }
}

@MainActor
private extension TrackingAutomationStore {
    /// Builds the store for tests of its preference and bookkeeping, which send no pause or resume request.
    convenience init(repository: any TrackingPreferencesRepository) {
        self.init(repository: repository, client: FakeClient(), clock: FakeClock(), tracking: TrackingStore())
    }
}

private final class PreferenceChanges {
    var count = 0
}
