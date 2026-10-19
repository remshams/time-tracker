import Foundation

public struct TaskRenamePresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let taskID: String?
    public let originalName: String
    public let name: String
    public let isSubmitting: Bool
    public let error: String?
    public let canEditName: Bool
    public let canSubmit: Bool
}

@MainActor
final class TaskRenameState {
    struct Request {
        let taskID: String
        let originalName: String
        let name: String
    }

    private(set) var isPresented = false
    private(set) var taskID: String?
    private(set) var name = ""
    private(set) var isSubmitting = false
    private(set) var error: String?
    private var originalName = ""

    var presentation: TaskRenamePresentation {
        let desired = TaskNameEditingPolicy.normalized(name)
        return TaskRenamePresentation(
            isPresented: isPresented, taskID: taskID, originalName: originalName, name: name,
            isSubmitting: isSubmitting, error: error,
            canEditName: !isSubmitting,
            canSubmit: isPresented && !isSubmitting && !desired.isEmpty && desired != originalName)
    }

    func open(_ task: TaskItem?) {
        guard !isPresented, let task else { return }
        taskID = task.id
        originalName = task.name
        name = task.name
        error = nil
        isPresented = true
    }

    func updateName(_ value: String) {
        guard isPresented, presentation.canEditName, value != name else { return }
        name = value
        error = nil
    }

    func cancel() {
        guard !isSubmitting else { return }
        reset()
    }

    func submit() -> Request? {
        guard presentation.canSubmit, let taskID else { return nil }
        if let message = TaskNameEditingPolicy.transportError(name) {
            error = message
            return nil
        }
        isSubmitting = true
        error = nil
        return Request(taskID: taskID, originalName: originalName, name: name)
    }

    func fail(_ failure: Error) {
        isSubmitting = false
        error = failure.localizedDescription
    }

    func reset() {
        isPresented = false
        taskID = nil
        name = ""
        originalName = ""
        isSubmitting = false
        error = nil
    }
}
