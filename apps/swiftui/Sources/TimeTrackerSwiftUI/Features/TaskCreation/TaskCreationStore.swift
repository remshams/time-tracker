import Combine
import TrackerClient

@MainActor
final class TaskCreationStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onTaskCreationChange = { [weak self] in self?.objectWillChange.send() }
    }

    var state: TaskCreationPresentation { session.taskCreation }
    var canOpen: Bool { session.canOpenTaskCreation }

    func open() { session.openTaskCreation() }
    func setName(_ name: String) { session.setTaskCreationName(name) }
    func cancel() { session.cancelTaskCreation() }
    func submit() { session.submitTaskCreation() }
}

// Keep publisher access on the main actor with compilers that support isolated conformances.
#if compiler(>=6.2)
    extension TaskCreationStore: @MainActor ObservableObject {}
#else
    extension TaskCreationStore: ObservableObject {}
#endif
