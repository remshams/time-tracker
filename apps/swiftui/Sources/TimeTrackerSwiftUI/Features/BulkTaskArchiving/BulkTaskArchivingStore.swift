import Combine
import TrackerClient

@MainActor
final class BulkTaskArchivingStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        self.presentation = presentation
        presentation.onBulkTaskArchivingChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: BulkTaskArchivingPresentation { session.bulkTaskArchiving }
    var sheetContent: BulkTaskArchivingPresentation { presentation.bulkTaskArchivingSheetContent }
    var canOpen: Bool { session.canOpenBulkTaskArchiving }
    func open() { session.openBulkTaskArchiving() }
    func updateDays(_ text: String) { session.updateBulkArchiveDays(text) }
    func refresh() { session.refreshBulkArchivePreview() }
    func cancel() { session.cancelBulkTaskArchiving() }
    func submit() { session.submitBulkTaskArchiving() }
}

#if compiler(>=6.2)
    extension BulkTaskArchivingStore: @MainActor ObservableObject {}
#else
    extension BulkTaskArchivingStore: ObservableObject {}
#endif
