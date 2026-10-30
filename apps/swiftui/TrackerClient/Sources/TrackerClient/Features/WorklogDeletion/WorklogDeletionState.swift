import Foundation

public struct WorklogDeletionPresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let taskName: String
    public let original: WorklogItem?
    public let isSubmitting: Bool
    public let error: String?
    public let canSubmit: Bool
}

final class WorklogDeletionState {
    private(set) var original: WorklogItem?
    private var taskName = ""
    private var isPresented = false
    private var isSubmitting = false
    private var error: String?

    var presentation: WorklogDeletionPresentation {
        WorklogDeletionPresentation(
            isPresented: isPresented, taskName: taskName, original: original,
            isSubmitting: isSubmitting, error: error,
            canSubmit: isPresented && !isSubmitting)
    }

    func open(_ worklog: WorklogItem, taskName: String) {
        // Running worklogs cannot be deleted. Stop the timer first.
        guard !isPresented, worklog.end != nil else { return }
        original = worklog
        self.taskName = taskName
        isPresented = true
        error = nil
    }

    func submit() -> WorklogItem? {
        guard presentation.canSubmit, let original else { return nil }
        isSubmitting = true
        error = nil
        return original
    }

    func fail(_ failure: Error) {
        isSubmitting = false
        error = failure.localizedDescription
    }

    func cancel() {
        guard !isSubmitting else { return }
        reset()
    }

    func reset() {
        original = nil
        taskName = ""
        isPresented = false
        isSubmitting = false
        error = nil
    }
}
