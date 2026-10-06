import Combine
import TrackerClient

@MainActor
final class TaskArchivingStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        self.presentation = presentation
        presentation.onTaskArchivingChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: TaskArchivingPresentation { session.taskArchiving }
    var sheetContent: TaskArchivingPresentation { presentation.taskArchivingSheetContent }

    func canArchive(taskID: String) -> Bool { session.canArchiveTask(taskID: taskID) }
    func canUnarchive(taskID: String) -> Bool { session.canUnarchiveTask(taskID: taskID) }
    func openArchive(taskID: String) { session.openTaskArchive(taskID: taskID) }
    func unarchive(taskID: String) { session.unarchiveTask(taskID: taskID) }
    func cancel() { session.cancelTaskArchiving() }
    func submit() { session.submitTaskArchiving() }
    func reopen() { session.reopenTaskArchiving() }
    func reviewLatest() { session.reviewLatestTaskArchiving() }
}

#if compiler(>=6.2)
extension TaskArchivingStore: @MainActor ObservableObject {}
#else
extension TaskArchivingStore: ObservableObject {}
#endif
