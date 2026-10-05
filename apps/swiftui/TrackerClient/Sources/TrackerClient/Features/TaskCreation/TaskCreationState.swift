import Foundation

public struct TaskCreationPresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let name: String
    public let isSubmitting: Bool
    public let error: String?
    public let canEditName: Bool
    public let canSubmit: Bool
}

@MainActor
final class TaskCreationState {
    struct Intent {
        let name: String
        let occurredAt: String
    }

    private(set) var isPresented = false
    private(set) var name = ""
    private(set) var isSubmitting = false
    private(set) var error: String?
    private(set) var intent: Intent?
    var pending = false

    var presentation: TaskCreationPresentation {
        TaskCreationPresentation(isPresented: isPresented, name: name,
                                 isSubmitting: isSubmitting, error: error,
                                 canEditName: !isSubmitting && intent == nil,
                                 canSubmit: isPresented && !isSubmitting &&
                                    !TaskNameEditingPolicy.normalized(name).isEmpty)
    }

    var blocksConnectionChange: Bool { isSubmitting || intent != nil }

    static func requiresRecovery(_ error: Error) -> Bool {
        TaskNameEditingPolicy.requiresRecovery(error)
    }

    func open() {
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
        guard presentation.canSubmit else { return false }
        if let message = TaskNameEditingPolicy.transportError(name) {
            error = message
            return false
        }
        if intent == nil { intent = Intent(name: name, occurredAt: occurredAt) }
        isSubmitting = true
        pending = true
        error = nil
        return true
    }

    func fail(_ failure: Error, retainIntent: Bool) {
        isSubmitting = false
        pending = false
        error = failure.localizedDescription
        if !retainIntent { intent = nil }
    }

    func reset() {
        isPresented = false
        name = ""
        isSubmitting = false
        error = nil
        intent = nil
        pending = false
    }
}
