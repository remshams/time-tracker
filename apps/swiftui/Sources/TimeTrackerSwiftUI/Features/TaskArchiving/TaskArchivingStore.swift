import TrackerClient

@MainActor
final class TaskArchivingStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: TaskArchivingPresentation { presentation.taskArchiving }
    var sheetContent: TaskArchivingPresentation { presentation.taskArchivingSheetContent }

    func canArchive(taskID: String) -> Bool { presentation.canArchiveTask(taskID: taskID) }
    func canUnarchive(taskID: String) -> Bool { presentation.canUnarchiveTask(taskID: taskID) }
    func openArchive(taskID: String) { session.openTaskArchive(taskID: taskID) }
    func unarchive(taskID: String) { session.unarchiveTask(taskID: taskID) }
    func cancel() { session.cancelTaskArchiving() }
    func submit() { session.submitTaskArchiving() }
    func reopen() { session.reopenTaskArchiving() }
    func reviewLatest() { session.reviewLatestTaskArchiving() }
}
