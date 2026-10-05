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
    struct Intent {
        let taskID: String
        let originalName: String
        let name: String
        let occurredAt: String

        var desiredName: String { TaskNameEditingPolicy.normalized(name) }
    }

    private(set) var isPresented = false
    private(set) var taskID: String?
    private(set) var name = ""
    private(set) var isSubmitting = false
    private(set) var error: String?
    private(set) var intent: Intent?
    private var originalName = ""
    private var pending = false
    private var requiresReview = false

    var presentation: TaskRenamePresentation {
        let desired = TaskNameEditingPolicy.normalized(name)
        return TaskRenamePresentation(isPresented: isPresented, taskID: taskID, originalName: originalName, name: name,
                                      isSubmitting: isSubmitting, error: error,
                                      canEditName: !isSubmitting && intent == nil && !requiresReview,
                                      canSubmit: isPresented && !isSubmitting && !requiresReview &&
                                        !desired.isEmpty && desired != originalName)
    }

    var blocksConnectionChange: Bool { isSubmitting || intent != nil }
    var hasPendingIntent: Bool { pending }

    func open(_ task: TaskItem?) {
        if intent != nil || isPresented {
            isPresented = true
            return
        }
        guard let task else { return }
        taskID = task.id
        originalName = task.name
        name = task.name
        error = nil
        requiresReview = false
        isPresented = true
    }

    func updateName(_ value: String) {
        guard isPresented, presentation.canEditName, value != name else { return }
        name = value
        error = nil
    }

    func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
        if intent == nil { reset() }
    }

    func submit(at occurredAt: String) -> Bool {
        guard presentation.canSubmit, let taskID else { return false }
        if let message = TaskNameEditingPolicy.transportError(name) {
            error = message
            return false
        }
        if intent == nil {
            intent = Intent(taskID: taskID, originalName: originalName, name: name, occurredAt: occurredAt)
        }
        isSubmitting = true
        pending = true
        error = nil
        return true
    }

    func takePendingIntent() -> Intent? {
        guard pending, let intent else { return nil }
        pending = false
        return intent
    }

    func deferUntilWake() { pending = true }

    func fail(_ failure: Error, retainIntent: Bool) {
        isSubmitting = false
        pending = false
        error = failure.localizedDescription
        if !retainIntent { intent = nil }
    }

    func requireReview(_ message: String) {
        intent = nil
        isSubmitting = false
        pending = false
        requiresReview = true
        error = message
    }

    func reset() {
        isPresented = false
        taskID = nil
        name = ""
        originalName = ""
        isSubmitting = false
        error = nil
        intent = nil
        pending = false
        requiresReview = false
    }
}
