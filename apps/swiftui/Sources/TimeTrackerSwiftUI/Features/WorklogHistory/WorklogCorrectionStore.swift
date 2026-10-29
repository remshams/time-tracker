import Foundation
import TrackerClient

final class WorklogCorrectionStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: WorklogCorrectionPresentation { presentation.worklogCorrection }
    var sheetContent: WorklogCorrectionPresentation { presentation.worklogCorrectionSheetContent }
    var canOpen: Bool { presentation.canOpenWorklogCorrection }

    func open(worklogID: String) { session.openWorklogCorrection(worklogID: worklogID) }
    func setStart(_ date: Date) { session.setWorklogCorrectionStart(date) }
    func setEnd(_ date: Date) { session.setWorklogCorrectionEnd(date) }
    func cancel() { session.cancelWorklogCorrection() }
    func submit() { session.submitWorklogCorrection() }
}
