import Foundation

public enum TaskArchivingAction: Equatable, Sendable {
    case archive
    case unarchive
}

public struct TaskArchivingPresentation: Equatable, Sendable {
    public let taskID: String?
    public let taskName: String
    public let action: TaskArchivingAction
    public let isPresented: Bool
    public let isSubmitting: Bool
    public let error: String?
    public let canSubmit: Bool
}

@MainActor
final class TaskArchivingState {
    struct Request {
        let taskID: String
        let taskName: String
        let action: TaskArchivingAction
    }

    private(set) var original: TaskItem?
    private(set) var action: TaskArchivingAction = .archive
    private(set) var isPresented = false
    private(set) var isSubmitting = false
    private(set) var error: String?

    var presentation: TaskArchivingPresentation {
        TaskArchivingPresentation(
            taskID: original?.id, taskName: original?.name ?? "", action: action,
            isPresented: isPresented, isSubmitting: isSubmitting, error: error,
            canSubmit: isPresented && original != nil && !isSubmitting)
    }

    var isOpen: Bool { isPresented || isSubmitting }

    func open(_ task: TaskItem, action: TaskArchivingAction) {
        guard !isOpen else { return }
        original = task
        self.action = action
        isPresented = action == .archive
        error = nil
    }

    func cancel() {
        guard !isSubmitting else { return }
        reset()
    }

    func submit() -> Request? {
        guard !isSubmitting, let original else { return nil }
        isSubmitting = true
        error = nil
        return Request(taskID: original.id, taskName: original.name, action: action)
    }

    /// Shows the error in the dialog, including for an unarchive that started without one.
    func fail(_ failure: Error) {
        isSubmitting = false
        isPresented = true
        error = failure.localizedDescription
    }

    func reset() {
        original = nil
        action = .archive
        isPresented = false
        isSubmitting = false
        error = nil
    }
}
