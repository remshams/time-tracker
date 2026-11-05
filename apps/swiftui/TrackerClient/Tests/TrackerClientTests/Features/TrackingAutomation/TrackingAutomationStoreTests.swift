import XCTest
@testable import TrackerClient

final class TrackingAutomationStoreTests: XCTestCase {
    @MainActor
    func testEnabledNotifiesItsReadersOnlyWhenItChanges() async {
        let store = TrackingAutomationStore(preferences: TrackingPreferences())
        let enabled = ObservationProbe { _ = store.enabled }
        let status = ObservationProbe { _ = store.statusText }

        XCTAssertFalse(store.setEnabled(false))
        XCTAssertFalse(enabled.consume())
        XCTAssertTrue(store.setEnabled(true))
        XCTAssertTrue(store.enabled)
        XCTAssertTrue(enabled.consume())
        XCTAssertFalse(status.consume())
    }

    @MainActor
    func testStatusTextChangesOnlyWhenPublishedAndNotifiesOnlyForNewText() async {
        let store = TrackingAutomationStore(preferences: TrackingPreferences(pauseOnScreenLock: true))
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
