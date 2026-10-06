import Combine
import Foundation
import TrackerClient

@MainActor
final class WorklogCorrectionStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onWorklogCorrectionChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: WorklogCorrectionPresentation { session.worklogCorrection }
    var canOpen: Bool { session.canOpenWorklogCorrection }

    func open(worklogID: String) { session.openWorklogCorrection(worklogID: worklogID) }
    func setStart(_ date: Date) { session.setWorklogCorrectionStart(date) }
    func setEnd(_ date: Date) { session.setWorklogCorrectionEnd(date) }
    func cancel() { session.cancelWorklogCorrection() }
    func submit() { session.submitWorklogCorrection() }
    func reviewLatest() { session.reviewLatestWorklogCorrection() }
}

#if compiler(>=6.2)
extension WorklogCorrectionStore: @MainActor ObservableObject {}
#else
extension WorklogCorrectionStore: ObservableObject {}
#endif
