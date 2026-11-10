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
            _ = catalog.acceptTaskList(
                [firstTask, secondTask, archivedTask], previousActive: nil, active: activeWorklog)
            tracking.acceptTaskList(active: activeWorklog, clock: FakeClock())
        }
    }

    @MainActor
    func testConfirmedCurrentStateAllowsStartAndStop() async {
        let stores = Stores()

        XCTAssertTrue(stores.availability.canSend)
        XCTAssertTrue(stores.availability.canStart(taskID: secondTask.id))
        XCTAssertTrue(stores.availability.canStop)
    }

    @MainActor
    func testUnconfirmedStateBlocksStartAndStop() async {
        let stores = Stores()
        let unconfirmed = ConnectionStore(settings: .local)
        let availability = TrackingAvailability(
            connection: unconfirmed, catalog: stores.catalog, tracking: stores.tracking)

        XCTAssertFalse(availability.canSend)
        XCTAssertFalse(availability.canStart(taskID: secondTask.id))
        XCTAssertFalse(availability.canStop)
    }

    @MainActor
    func testStaleStateBlocksStartAndStopUntilConfirmedAgain() async {
        let stores = Stores()

        stores.connection.recordFailure(BridgeFailure(message: "Tracker offline", kind: "unavailable"))
        XCTAssertFalse(stores.availability.canSend)
        XCTAssertFalse(stores.availability.canStart(taskID: secondTask.id))
        XCTAssertFalse(stores.availability.canStop)

        stores.connection.acceptTaskList()
        XCTAssertTrue(stores.availability.canSend)
    }

    @MainActor
    func testSubmittingBlocksStartAndStopUntilItEnds() async {
        let stores = Stores()

        stores.tracking.beginSubmitting()
        XCTAssertFalse(stores.availability.canSend)
        XCTAssertFalse(stores.availability.canStart(taskID: secondTask.id))
        XCTAssertFalse(stores.availability.canStop)

        stores.tracking.endSubmitting()
        XCTAssertTrue(stores.availability.canSend)
    }

    @MainActor
    func testOnlyAKnownUnarchivedTaskThatIsNotRunningCanStart() async {
        let stores = Stores()

        XCTAssertFalse(stores.availability.canStart(taskID: firstTask.id), "The running task can't start again.")
        XCTAssertFalse(stores.availability.canStart(taskID: archivedTask.id))
        XCTAssertFalse(stores.availability.canStart(taskID: "missing-task"))

        stores.tracking.acceptTaskList(active: nil, clock: FakeClock())
        XCTAssertTrue(stores.availability.canStart(taskID: firstTask.id))
    }

    @MainActor
    func testStopNeedsARunningWorklog() async {
        let stores = Stores()

        stores.tracking.acceptTaskList(active: nil, clock: FakeClock())

        XCTAssertTrue(stores.availability.canSend)
        XCTAssertFalse(stores.availability.canStop)
    }

    @MainActor
    func testReadersObserveTheStoresBehindAChange() async {
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

        _ = stores.catalog.acceptTaskList([firstTask], previousActive: activeWorklog, active: activeWorklog)
        XCTAssertTrue(probe.consume())
    }
}
