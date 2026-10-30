import TrackerClient

final class TaskRenameStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: TaskRenamePresentation { presentation.taskRename }
    var canOpen: Bool { presentation.canOpenTaskRename }

    func open(taskID: String) { session.openTaskRename(taskID: taskID) }
    func setName(_ name: String) { session.setTaskRenameName(name) }
    func cancel() { session.cancelTaskRename() }
    func submit() { session.submitTaskRename() }
}
