import Foundation

public struct TaskCreationPresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let name: String
    public let isSubmitting: Bool
    public let error: String?
    public let canSubmit: Bool
}

@MainActor
final class TaskCreationState {
    private(set) var isPresented = false
    private(set) var name = ""
    private(set) var isSubmitting = false
    private(set) var error: String?

    var presentation: TaskCreationPresentation {
        TaskCreationPresentation(
            isPresented: isPresented, name: name,
            isSubmitting: isSubmitting, error: error,
            canSubmit: isPresented && !isSubmitting && !TaskNameEditingPolicy.normalized(name).isEmpty)
    }

    func open() {
        isPresented = true
    }

    func updateName(_ value: String) {
        guard isPresented, !isSubmitting, value != name else { return }
        name = value
        error = nil
    }

    func cancel() {
        guard !isSubmitting else { return }
        reset()
    }

    /// Returns the name to send, or nil when the draft cannot be submitted.
    func submit() -> String? {
        guard presentation.canSubmit else { return nil }
        if let message = TaskNameEditingPolicy.transportError(name) {
            error = message
            return nil
        }
        isSubmitting = true
        error = nil
        return name
    }

    func fail(_ failure: Error) {
        isSubmitting = false
        error = failure.localizedDescription
    }

    func reset() {
        isPresented = false
        name = ""
        isSubmitting = false
        error = nil
    }
}
