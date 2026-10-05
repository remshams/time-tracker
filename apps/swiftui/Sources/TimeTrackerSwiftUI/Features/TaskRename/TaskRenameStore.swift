import Combine
import TrackerClient

@MainActor
final class TaskRenameStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onTaskRenameChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: TaskRenamePresentation { session.taskRename }
    var canOpen: Bool { session.canOpenTaskRename }

    func open(taskID: String) { session.openTaskRename(taskID: taskID) }
    func setName(_ name: String) { session.setTaskRenameName(name) }
    func cancel() { session.cancelTaskRename() }
    func submit() { session.submitTaskRename() }
}

// Keep publisher access on the main actor with compilers that support isolated conformances.
#if compiler(>=6.2)
extension TaskRenameStore: @MainActor ObservableObject {}
#else
extension TaskRenameStore: ObservableObject {}
#endif
