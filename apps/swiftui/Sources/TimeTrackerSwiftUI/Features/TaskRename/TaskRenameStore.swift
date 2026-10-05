import Combine
import TrackerClient

@MainActor
final class TaskRenameStore: ObservableObject {
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
