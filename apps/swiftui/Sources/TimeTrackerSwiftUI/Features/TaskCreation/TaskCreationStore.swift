import TrackerClient

@MainActor
final class TaskCreationStore {
    private let session: TrackerSession
    private let presentation: TrackerPresentation

    init(session: TrackerSession, presentation: TrackerPresentation) {
        self.session = session
        self.presentation = presentation
    }

    var state: TaskCreationPresentation { presentation.taskCreation }
    var canOpen: Bool { presentation.canOpenTaskCreation && !state.isPresented }

    func open() { session.openTaskCreation() }
    func setName(_ name: String) { session.setTaskCreationName(name) }
    func cancel() { session.cancelTaskCreation() }
    func submit() { session.submitTaskCreation() }
}
