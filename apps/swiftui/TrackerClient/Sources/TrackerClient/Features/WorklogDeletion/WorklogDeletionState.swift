import Foundation

public struct WorklogDeletionPresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let taskName: String
    public let original: WorklogItem?
    public let isSubmitting: Bool
    public let error: String?
    public let canSubmit: Bool
    public let requiresReview: Bool
    public let latest: WorklogItem?
}

@MainActor
final class WorklogDeletionState {
    struct Intent {
        let expected: WorklogItem
        let historyPageLimit: Int

        func matches(_ worklog: WorklogItem) -> Bool {
            worklog.id == expected.id && worklog.taskId == expected.taskId
                && sameWorklogTimestamp(worklog.start, expected.start)
                && sameWorklogTimestamp(worklog.end, expected.end)
        }
    }

    private(set) var original: WorklogItem?
    private var taskName = ""
    private var historyPageLimit = 2
    private var isPresented = false
    private var isSubmitting = false
    private var error: String?
    private var requiresReview = false
    private var latest: WorklogItem?
    private(set) var intent: Intent?
    private(set) var pending = false
    private(set) var mayHaveCommitted = false

    var blocksConnectionChange: Bool { isPresented || intent != nil }
    var presentation: WorklogDeletionPresentation {
        WorklogDeletionPresentation(
            isPresented: isPresented, taskName: taskName, original: original,
            isSubmitting: isSubmitting, error: error,
            canSubmit: isPresented && !isSubmitting && !requiresReview,
            requiresReview: requiresReview, latest: latest)
    }

    func open(_ worklog: WorklogItem, taskName: String, historyPageLimit: Int) {
        if isPresented || intent != nil { isPresented = true; return }
        // Running worklogs cannot be deleted. Stop the timer first.
        guard worklog.end != nil else { return }
        original = worklog
        self.taskName = taskName
        self.historyPageLimit = historyPageLimit
        isPresented = true
        error = nil
    }

    func submit() -> Bool {
        guard presentation.canSubmit, let original else { return false }
        if intent == nil { intent = Intent(expected: original, historyPageLimit: historyPageLimit) }
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
    func commandStarted() { mayHaveCommitted = true }

    func fail(_ failure: Error, retainIntent: Bool) {
        isSubmitting = false
        pending = false
        error = failure.localizedDescription
        if !retainIntent { intent = nil; mayHaveCommitted = false }
    }

    func requireReview(latest: WorklogItem?, message: String) {
        self.latest = latest
        requiresReview = true
        isSubmitting = false
        pending = false
        intent = nil
        mayHaveCommitted = false
        error = message
    }

    func reviewLatest(taskName: String?) {
        guard requiresReview, let latest, latest.end != nil else { return }
        original = latest
        if let taskName { self.taskName = taskName }
        self.latest = nil
        requiresReview = false
        error = nil
    }

    func observe(_ worklog: WorklogItem) {
        guard isPresented, !isSubmitting, intent == nil, let original,
            worklog.id == original.id, worklog != original
        else { return }
        requireReview(latest: worklog, message: "This worklog changed. Review it before deleting.")
    }

    func cancel() {
        guard !isSubmitting else { return }
        if intent != nil { isPresented = false } else { reset() }
    }

    func reset() {
        original = nil
        taskName = ""
        isPresented = false
        isSubmitting = false
        error = nil
        requiresReview = false
        latest = nil
        intent = nil
        pending = false
        mayHaveCommitted = false
    }
}
