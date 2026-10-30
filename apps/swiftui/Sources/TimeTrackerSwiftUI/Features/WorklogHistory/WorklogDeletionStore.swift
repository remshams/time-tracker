import Foundation
import TrackerClient

final class WorklogDeletionStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: WorklogDeletionPresentation { presentation.worklogDeletion }
    var sheetContent: WorklogDeletionPresentation { presentation.worklogDeletionSheetContent }
    var canOpen: Bool { presentation.canOpenWorklogDeletion }

    func open(worklogID: String) { session.openWorklogDeletion(worklogID: worklogID) }
    func cancel() { session.cancelWorklogDeletion() }
    func submit() { session.submitWorklogDeletion() }
}
