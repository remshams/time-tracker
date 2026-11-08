import XCTest
@testable import TrackerClient

final class TrackingAvailabilityTests: XCTestCase {
    @MainActor
    private final class Stores {
        let connection = ConnectionStore(settings: .local)
        let catalog = TaskCatalogStore()
        let tracking = TrackingStore()
        var availability: TrackingAvailability {
            TrackingAvailability(connection: connection, catalog: catalog, tracking: tracking)
        }

        /// A confirmed connection with `firstTask` running, so every check passes until a test changes one input.
        init() {
            connection.acceptTaskList()
            setRunning(activeWorklog)
        }

        func setRunning(_ worklog: WorklogItem?) {
            let tasks = [firstTask, secondTask, archivedTask]
            _ = catalog.apply(tasks, previousActive: tracking.active, active: worklog)
            tracking.apply(worklog, clock: FakeClock())
        }
    }

    @MainActor
    func testConfirmedCurrentStateAllowsStartAndStop() {
        let stores = Stores()

        XCTAssertTrue(stores.availability.canSend)
        XCTAssertTrue(stores.availability.canStart(taskID: secondTask.id))
        XCTAssertTrue(stores.availability.canStop)
    }

    @MainActor
    func testUnconfirmedStateBlocksStartAndStop() {
        let stores = Stores()
        let unconfirmed = ConnectionStore(settings: .local)
        let availability = TrackingAvailability(
            connection: unconfirmed, catalog: stores.catalog, tracking: stores.tracking)

        XCTAssertFalse(availability.canSend)
        XCTAssertFalse(availability.canStart(taskID: secondTask.id))
        XCTAssertFalse(availability.canStop)
    }

    @MainActor
    func testStaleStateBlocksStartAndStopUntilConfirmedAgain() {
        let stores = Stores()

        stores.connection.recordFailure(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        XCTAssertFalse(stores.availability.canSend)
        XCTAssertFalse(stores.availability.canStart(taskID: secondTask.id))
        XCTAssertFalse(stores.availability.canStop)

        stores.connection.acceptTaskList()
        XCTAssertTrue(stores.availability.canSend)
    }

    @MainActor
    func testSubmittingBlocksStartAndStopUntilItEnds() {
        let stores = Stores()

        stores.tracking.beginSubmitting()
        XCTAssertFalse(stores.availability.canSend)
        XCTAssertFalse(stores.availability.canStart(taskID: secondTask.id))
        XCTAssertFalse(stores.availability.canStop)

        stores.tracking.endSubmitting()
        XCTAssertTrue(stores.availability.canSend)
    }

    @MainActor
    func testOnlyAKnownUnarchivedTaskThatIsNotRunningCanStart() {
        let stores = Stores()

        XCTAssertFalse(stores.availability.canStart(taskID: firstTask.id), "The running task can't start again.")
        XCTAssertFalse(stores.availability.canStart(taskID: archivedTask.id))
        XCTAssertFalse(stores.availability.canStart(taskID: "missing-task"))

        stores.setRunning(nil)
        XCTAssertTrue(stores.availability.canStart(taskID: firstTask.id))
        XCTAssertFalse(stores.availability.canStart(taskID: archivedTask.id))
    }

    @MainActor
    func testStopNeedsARunningWorklog() {
        let stores = Stores()

        stores.setRunning(nil)

        XCTAssertTrue(stores.availability.canSend)
        XCTAssertFalse(stores.availability.canStop)
    }

    @MainActor
    func testReadersObserveTheStoresBehindAChange() {
        let stores = Stores()
        let availability = stores.availability
        let probe = ObservationProbe {
            _ = availability.canStop
            _ = availability.canStart(taskID: secondTask.id)
        }
        XCTAssertFalse(probe.consume())

        stores.tracking.beginSubmitting()
        XCTAssertTrue(probe.consume())
        stores.tracking.endSubmitting()
        XCTAssertTrue(probe.consume())

        stores.connection.recordFailure(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        XCTAssertTrue(probe.consume())
        stores.connection.acceptTaskList()
        XCTAssertTrue(probe.consume())

        _ = stores.catalog.apply([firstTask], previousActive: activeWorklog, active: activeWorklog)
        XCTAssertTrue(probe.consume())
    }
}
