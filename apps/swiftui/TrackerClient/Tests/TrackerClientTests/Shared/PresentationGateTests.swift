import Foundation
import XCTest
@testable import TrackerClient

@MainActor
final class PresentationGateTests: XCTestCase {
    private struct Dialog {
        let name: String
        let canOpen: () -> Bool
        let open: () -> Void
        let cancel: () -> Void
    }

    private let resources = TaskListResources(tasks: [firstTask, secondTask, archivedTask], active: nil)

    func testEachHolderKeepsTheGateClaimedUntilItReleasesIt() async {
        let gate = PresentationGate()
        let sheet = NSObject()
        let alert = NSObject()

        gate.update(sheet, isShown: true)
        gate.update(alert, kind: .alert, isShown: true)
        XCTAssertTrue(gate.isClaimed)
        XCTAssertTrue(gate.showsSheet)

        gate.update(sheet, isShown: false)
        XCTAssertTrue(gate.isClaimed)
        XCTAssertFalse(gate.showsSheet)

        gate.update(alert, kind: .alert, isShown: false)
        XCTAssertFalse(gate.isClaimed)
        XCTAssertFalse(gate.showsSheet)
    }

    func testAnUnchangedClaimDoesNotNotifyObservers() async {
        let gate = PresentationGate()
        let holder = NSObject()
        let probe = ObservationProbe { _ = gate.isClaimed }

        gate.update(holder, isShown: false)
        XCTAssertFalse(probe.consume())
        gate.update(holder, isShown: true)
        XCTAssertTrue(probe.consume())
        gate.update(holder, isShown: true)
        XCTAssertFalse(probe.consume())
    }

    func testAnOpenDialogBlocksEveryDialogUntilItCloses() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await start(fixture)
        let dialogs = dialogs(fixture)
        let gate = fixture.features.presentationGate

        for dialog in dialogs {
            XCTAssertFalse(gate.isClaimed, "The gate is claimed before \(dialog.name) opens.")
            dialog.open()
            XCTAssertTrue(gate.showsSheet, "\(dialog.name) didn't claim the gate.")
            for other in dialogs {
                XCTAssertFalse(other.canOpen(), "\(other.name) can open while \(dialog.name) shows.")
            }
            dialog.cancel()
            for other in dialogs {
                XCTAssertTrue(other.canOpen(), "\(other.name) stays blocked after \(dialog.name) closed.")
            }
        }
        XCTAssertFalse(gate.isClaimed)
    }

    func testTheTrackingErrorBlocksEveryDialogUntilItIsDismissed() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await start(fixture)
        let dialogs = dialogs(fixture)
        let gate = fixture.features.presentationGate

        fixture.features.tracking.recordFailure(BridgeFailure(message: "Start rejected"))
        fixture.features.tracking.showRecordedError()
        XCTAssertTrue(gate.isClaimed)
        XCTAssertFalse(gate.showsSheet)
        for dialog in dialogs {
            XCTAssertFalse(dialog.canOpen(), "\(dialog.name) can open while the tracking error shows.")
        }
        fixture.features.taskCreation.open()
        XCTAssertFalse(fixture.features.taskCreation.isPresented)

        fixture.features.tracking.dismissError()
        XCTAssertFalse(gate.isClaimed)
        for dialog in dialogs {
            XCTAssertTrue(dialog.canOpen(), "\(dialog.name) stays blocked after the tracking error closed.")
        }
    }

    /// Loads the tasks and a completed worklog of the selected task, so every dialog can open.
    private func start(_ fixture: Fixture) async throws {
        try await fixture.start(resources)
        fixture.features.worklogHistory.reload()
        let page = try await fixture.client.next()
        page.succeed(HistoryPage(worklogs: [oldWorklog], nextCursor: nil, reset: false))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.worklogHistory.worklogs, [oldWorklog])
        XCTAssertEqual(fixture.features.taskCatalog.selectedTask, firstTask)
    }

    private func dialogs(_ fixture: Fixture) -> [Dialog] {
        let features = fixture.features
        return [
            Dialog(
                name: "Task creation", canOpen: { features.taskCreation.canOpen },
                open: { features.taskCreation.open() }, cancel: { features.taskCreation.cancel() }),
            Dialog(
                name: "Task rename", canOpen: { features.taskRename.canOpen },
                open: { features.taskRename.open(taskID: firstTask.id) }, cancel: { features.taskRename.cancel() }),
            // Unarchiving shows no dialog, but it is blocked like one.
            Dialog(
                name: "Task archive",
                canOpen: {
                    features.taskArchiving.canArchive(taskID: firstTask.id)
                        && features.taskArchiving.canUnarchive(taskID: archivedTask.id)
                },
                open: { features.taskArchiving.openArchive(taskID: firstTask.id) },
                cancel: { features.taskArchiving.cancel() }),
            Dialog(
                name: "Bulk archive", canOpen: { features.bulkTaskArchiving.canOpen },
                open: { features.bulkTaskArchiving.open() }, cancel: { features.bulkTaskArchiving.cancel() }),
            Dialog(
                name: "Worklog correction", canOpen: { features.worklogCorrection.canOpen },
                open: { features.worklogCorrection.open(worklogID: oldWorklog.id) },
                cancel: { features.worklogCorrection.cancel() }),
            Dialog(
                name: "Worklog move", canOpen: { features.worklogMove.canOpen },
                open: { features.worklogMove.open(worklogID: oldWorklog.id) },
                cancel: { features.worklogMove.cancel() }),
            Dialog(
                name: "Worklog deletion", canOpen: { features.worklogDeletion.canOpen },
                open: { features.worklogDeletion.open(worklogID: oldWorklog.id) },
                cancel: { features.worklogDeletion.cancel() }),
        ]
    }
}
