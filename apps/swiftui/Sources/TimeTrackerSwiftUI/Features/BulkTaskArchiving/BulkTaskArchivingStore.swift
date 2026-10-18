import TrackerClient

@MainActor
final class BulkTaskArchivingStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: BulkTaskArchivingPresentation { presentation.bulkTaskArchiving }
    var sheetContent: BulkTaskArchivingPresentation { presentation.bulkTaskArchivingSheetContent }
    var canOpen: Bool { presentation.canOpenBulkTaskArchiving }
    func open() { session.openBulkTaskArchiving() }
    func updateDays(_ text: String) { session.updateBulkArchiveDays(text) }
    func refresh() { session.refreshBulkArchivePreview() }
    func cancel() { session.cancelBulkTaskArchiving() }
    func submit() { session.submitBulkTaskArchiving() }
}
