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
    func testSettingThePreferenceSavesOnlyChanges() async {
        let repository = MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true))
        let store = TrackingAutomationStore(repository: repository)
        XCTAssertTrue(store.enabled)

        store.setPauseOnScreenLock(true)
        XCTAssertEqual(repository.writes, [])
        store.setPauseOnScreenLock(false)
        XCTAssertFalse(store.enabled)
        XCTAssertEqual(repository.writes, [TrackingPreferences()])
        store.setPauseOnScreenLock(true)
        XCTAssertEqual(repository.writes, [TrackingPreferences(), TrackingPreferences(pauseOnScreenLock: true)])
    }

    @MainActor
    func testTurningThePreferenceOffDropsAPendingResume() async {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        XCTAssertNotNil(store.statusText)

        store.setPauseOnScreenLock(false)
        XCTAssertNil(store.statusText)
    }

    @MainActor
    func testStatusTextFollowsThePausedTaskAndNotifiesOnlyForNewText() async {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        let status = ObservationProbe { _ = store.statusText }
        let enabled = ObservationProbe { _ = store.enabled }
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertFalse(status.consume(), "Beginning a pause holds no task yet.")
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        XCTAssertEqual(store.statusText, "Paused while screen is locked")
        XCTAssertTrue(status.consume())

        store.acceptTaskList(active: nil)
        XCTAssertFalse(status.consume(), "An unchanged paused task must not redraw the status.")

        XCTAssertEqual(store.unlock(at: Date(timeIntervalSince1970: 90))?.taskID, firstTask.id)
        XCTAssertNil(store.statusText)
        XCTAssertTrue(status.consume())
        XCTAssertFalse(enabled.consume())
    }

    @MainActor
    func testAcceptedTaskListWithARunningWorklogDropsThePausedTask() async {
        let idle = pausedStore()
        idle.acceptTaskList(active: nil)
        XCTAssertEqual(idle.unlock(at: Date(timeIntervalSince1970: 90))?.taskID, firstTask.id)

        let tracking = pausedStore()
        tracking.acceptTaskList(active: activeWorklog)
        XCTAssertNil(tracking.unlock(at: Date(timeIntervalSince1970: 90)))
    }

    /// Returns a store whose screen-lock pause stopped `firstTask` while the screen is locked.
    @MainActor
    private func pausedStore() -> TrackingAutomationStore {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        return store
    }
}

@MainActor
private extension TrackingAutomationStore {
    /// Builds the store for tests of its preference and bookkeeping, which send no pause or resume request.
    convenience init(repository: any TrackingPreferencesRepository) {
        self.init(repository: repository, client: FakeClient(), clock: FakeClock(), tracking: TrackingStore())
    }
}
