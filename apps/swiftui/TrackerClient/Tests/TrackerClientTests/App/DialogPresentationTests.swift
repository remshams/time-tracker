import XCTest
@testable import TrackerClient
#if canImport(Combine)
    import Combine
#endif

final class DialogPresentationTests: XCTestCase {
    @MainActor
    func testRenameValidationUpdatesTheDisplayedSnapshotWithoutSendingACommand() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let rename = fixture.session.renameModel
        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("Invalid\u{0000}name")
        let operationCount = fixture.client.operations.count
        XCTAssertNil(rename.sheetContent.error)
        fixture.session.submitTaskRename()
        XCTAssertEqual(rename.state.error, "Task names must not contain control characters.")
        XCTAssertEqual(rename.sheetContent, rename.state)
        XCTAssertTrue(rename.state.isPresented)
        XCTAssertFalse(rename.state.isSubmitting)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
    }

    @MainActor
    func testNameDraftsStayVisibleDuringDismissalAndAreReplacedOnReopening() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        let creation = fixture.session.creationModel
        let rename = fixture.session.renameModel
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("A new task")
        let creationContent = creation.sheetContent
        XCTAssertEqual(creationContent.name, "A new task")
        XCTAssertTrue(creationContent.canSubmit)
        fixture.session.cancelTaskCreation()
        XCTAssertFalse(creation.state.isPresented)
        XCTAssertEqual(creation.sheetContent, creationContent)
        fixture.session.openTaskCreation()
        XCTAssertEqual(creation.sheetContent.name, "")
        fixture.session.cancelTaskCreation()

        fixture.session.openTaskRename()
        fixture.session.setTaskRenameName("A changed name")
        let renameContent = rename.sheetContent
        XCTAssertEqual(renameContent.name, "A changed name")
        XCTAssertTrue(renameContent.canSubmit)
        fixture.session.cancelTaskRename()
        XCTAssertFalse(rename.state.isPresented)
        XCTAssertEqual(rename.sheetContent, renameContent)
        fixture.session.openTaskRename()
        XCTAssertEqual(rename.sheetContent.name, firstTask.name)
        XCTAssertEqual(rename.sheetContent.originalName, firstTask.name)
    }

    @MainActor
    func testDialogModelsRemainTheSameInstancesAcrossSessionUpdates() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        let creation = fixture.session.creationModel
        let rename = fixture.session.renameModel
        let archiving = fixture.session.archivingModel
        let bulk = fixture.session.bulkArchivingModel
        let correction = fixture.session.correctionModel
        let move = fixture.session.moveModel
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        XCTAssertTrue(creation === fixture.session.creationModel)
        XCTAssertTrue(rename === fixture.session.renameModel)
        XCTAssertTrue(archiving === fixture.session.archivingModel)
        XCTAssertTrue(bulk === fixture.session.bulkArchivingModel)
        XCTAssertTrue(correction === fixture.session.correctionModel)
        XCTAssertTrue(move === fixture.session.moveModel)
        XCTAssertTrue(creation.canOpen)
        XCTAssertTrue(rename.canOpen)
        XCTAssertEqual(archiving.availability[firstTask.id], true)
        XCTAssertTrue(bulk.canOpen)
        XCTAssertTrue(correction.canOpen)
        XCTAssertTrue(move.canOpen)
        fixture.session.shutdown()
        XCTAssertFalse(creation.canOpen)
        XCTAssertFalse(rename.canOpen)
        XCTAssertEqual(archiving.availability[firstTask.id], false)
        XCTAssertFalse(bulk.canOpen)
        XCTAssertFalse(correction.canOpen)
        XCTAssertFalse(move.canOpen)
    }

    #if canImport(Combine)
        @MainActor
        func testSynchronousRoutingCanReadThePresentedMoveFromTheSessionBeforeItsModelChanges() async throws {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
            let model = fixture.session.moveModel
            var sessionSnapshots: [Bool] = []
            var modelSnapshots: [Bool] = []
            let subscription = model.objectWillChange.sink {
                sessionSnapshots.append(fixture.session.worklogMove.isPresented)
                modelSnapshots.append(model.state.isPresented)
            }
            defer { subscription.cancel() }
            fixture.session.openWorklogMove(worklogID: activeWorklog.id)
            XCTAssertEqual(sessionSnapshots.first, true)
            XCTAssertEqual(modelSnapshots.first, false)
            XCTAssertTrue(model.state.isPresented)
        }

        @MainActor
        func testPublishersIgnoreTimerTicksAndEqualDraftsButPublishValidationAndAvailability() async throws {
            let fixture = Fixture()
            defer { fixture.cleanup() }
            fixture.session.setWindowVisible(true)
            try await fixture.start(TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog))
            var creationChanges = 0
            var renameChanges = 0
            var bulkChanges = 0
            var archiveChanges = 0
            var correctionChanges = 0
            var moveChanges = 0
            let subscriptions = [
                fixture.session.creationModel.objectWillChange.sink { creationChanges += 1 },
                fixture.session.renameModel.objectWillChange.sink { renameChanges += 1 },
                fixture.session.bulkArchivingModel.objectWillChange.sink { bulkChanges += 1 },
                fixture.session.archivingModel.objectWillChange.sink { archiveChanges += 1 },
                fixture.session.correctionModel.objectWillChange.sink { correctionChanges += 1 },
                fixture.session.moveModel.objectWillChange.sink { moveChanges += 1 },
            ]
            defer { subscriptions.forEach { $0.cancel() } }
            let display = try XCTUnwrap(fixture.scheduler.display)
            for _ in 0..<3 {
                fixture.clock.now.addTimeInterval(1)
                fixture.clock.uptime += 1
                display.fire()
            }
            XCTAssertEqual(
                [creationChanges, renameChanges, bulkChanges, archiveChanges, correctionChanges, moveChanges],
                [0, 0, 0, 0, 0, 0])
            fixture.session.openTaskCreation()
            XCTAssertEqual(
                [creationChanges, renameChanges, bulkChanges, archiveChanges, correctionChanges, moveChanges],
                [1, 1, 1, 1, 1, 1])
            fixture.session.setTaskCreationName("Invalid\u{0000}name")
            XCTAssertEqual(creationChanges, 2)
            fixture.session.setTaskCreationName("Invalid\u{0000}name")
            XCTAssertEqual(creationChanges, 2)
            fixture.session.submitTaskCreation()
            XCTAssertEqual(creationChanges, 3)
            XCTAssertEqual(
                fixture.session.creationModel.sheetContent.error, "Task names must not contain control characters.")
            fixture.session.cancelTaskCreation()
            XCTAssertEqual(
                [creationChanges, renameChanges, bulkChanges, archiveChanges, correctionChanges, moveChanges],
                [4, 2, 2, 2, 2, 2])
        }
    #endif
}
