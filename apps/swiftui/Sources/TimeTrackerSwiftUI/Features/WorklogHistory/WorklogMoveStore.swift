import Combine
import Foundation
import TrackerClient

@MainActor
final class WorklogMoveStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        self.presentation = presentation
        presentation.onWorklogMoveChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: WorklogMovePresentation { session.worklogMove }
    var sheetContent: WorklogMovePresentation { presentation.worklogMoveSheetContent }
    var canOpen: Bool { session.canOpenWorklogMove }

    func open(worklogID: String) { session.openWorklogMove(worklogID: worklogID) }
    func setQuery(_ query: String) { session.setWorklogMoveQuery(query) }
    func select(taskID: String) { session.selectWorklogMoveDestination(taskID: taskID) }
    func moveSelection(by offset: Int) { session.moveWorklogDestinationSelection(by: offset) }
    func retryCandidates() { session.retryWorklogMoveCandidates() }
    func cancel() { session.cancelWorklogMove() }
    func submit() { session.submitWorklogMove() }
    func reviewLatest() { session.reviewLatestWorklogMove() }
}

#if compiler(>=6.2)
    extension WorklogMoveStore: @MainActor ObservableObject {}
#else
    extension WorklogMoveStore: ObservableObject {}
#endif
