import Foundation
import TrackerClient

@MainActor
final class WorklogMoveStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: WorklogMovePresentation { presentation.worklogMove }
    var sheetContent: WorklogMovePresentation { presentation.worklogMoveSheetContent }
    var canOpen: Bool { presentation.canOpenWorklogMove }

    func open(worklogID: String) { session.openWorklogMove(worklogID: worklogID) }
    func setQuery(_ query: String) { session.setWorklogMoveQuery(query) }
    func select(taskID: String) { session.selectWorklogMoveDestination(taskID: taskID) }
    func moveSelection(by offset: Int) { session.moveWorklogDestinationSelection(by: offset) }
    func retryCandidates() { session.retryWorklogMoveCandidates() }
    func cancel() { session.cancelWorklogMove() }
    func submit() { session.submitWorklogMove() }
}
