import Combine
import Foundation
import TrackerClient

@MainActor
final class WorklogDeletionStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        self.presentation = presentation
        presentation.onWorklogDeletionChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: WorklogDeletionPresentation { session.worklogDeletion }
    var sheetContent: WorklogDeletionPresentation { presentation.worklogDeletionSheetContent }
    var canOpen: Bool { session.canOpenWorklogDeletion }

    func open(worklogID: String) { session.openWorklogDeletion(worklogID: worklogID) }
    func cancel() { session.cancelWorklogDeletion() }
    func submit() { session.submitWorklogDeletion() }
    func reviewLatest() { session.reviewLatestWorklogDeletion() }
}

#if compiler(>=6.2)
    extension WorklogDeletionStore: @MainActor ObservableObject {}
#else
    extension WorklogDeletionStore: ObservableObject {}
#endif
