import Foundation

public enum TaskArchivingAction: Equatable, Sendable {
    case archive
    case unarchive

    var desiredArchived: Bool { self == .archive }
}

public struct TaskArchivingPresentation: Equatable, Sendable {
    public let taskID: String?
    public let taskName: String
    public let action: TaskArchivingAction
    public let isPresented: Bool
    public let isSubmitting: Bool
    public let error: String?
    public let canSubmit: Bool
    public let requiresReview: Bool
    public let latest: TaskItem?
    public let hasPendingAction: Bool
    public let hasUnresolvedIntent: Bool
}

@MainActor
final class TaskArchivingState {
    struct Intent {
        let taskID: String
        let taskName: String
        let originalArchived: Bool
        let action: TaskArchivingAction
        let occurredAt: String
    }

    enum Preflight { case apply, applied, review }

    private(set) var original: TaskItem?
    private(set) var action: TaskArchivingAction = .archive
    private(set) var isPresented = false
    private(set) var isSubmitting = false
    private(set) var error: String?
    private(set) var requiresReview = false
    private(set) var latest: TaskItem?
    private(set) var intent: Intent?
    private(set) var pending = false

    var presentation: TaskArchivingPresentation {
        TaskArchivingPresentation(taskID: original?.id, taskName: original?.name ?? "", action: action,
                                  isPresented: isPresented, isSubmitting: isSubmitting, error: error,
                                  canSubmit: isPresented && original != nil && !isSubmitting && !requiresReview,
                                  requiresReview: requiresReview, latest: latest,
                                  hasPendingAction: original != nil && !isPresented,
                                  hasUnresolvedIntent: intent != nil)
    }

    var blocksConnectionChange: Bool { isPresented || isSubmitting || intent != nil }
    var ownsPresentation: Bool { blocksConnectionChange }

    static func requiresRecovery(_ error: Error) -> Bool {
        guard let failure = error as? BridgeFailure else { return true }
        return failure.uncertain || failure.requiresRefresh ||
            failure.kind == "unavailable" || failure.kind == "protocol"
    }

    func open(_ task: TaskItem, action: TaskArchivingAction) {
        guard !ownsPresentation else { return }
        original = task
        self.action = action
        isPresented = action == .archive
        error = nil
        latest = nil
        requiresReview = false
    }

    func reopen() {
        guard original != nil else { return }
        isPresented = true
    }

    func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
        if intent == nil { reset() }
    }

    func submit(at occurredAt: String, immediate: Bool = false) -> Bool {
        guard !isSubmitting, !requiresReview, let original,
              immediate || presentation.canSubmit else { return false }
        if intent == nil {
            intent = Intent(taskID: original.id, taskName: original.name, originalArchived: original.archived,
                            action: action, occurredAt: occurredAt)
        }
        pending = true
        isSubmitting = true
        error = nil
        return true
    }

    func takePendingIntent() -> Intent? {
        guard pending, let intent else { return nil }
        pending = false
        return intent
    }

    func deferUntilWake() { pending = true }

    func preflight(_ intent: Intent, snapshot: TrackerSnapshot) -> Preflight {
        guard let task = snapshot.tasks.first(where: { $0.id == intent.taskID }) else {
            requireReview("The task no longer exists. Cancel and refresh the task list.", latest: nil)
            return .review
        }
        if responseMatches(snapshot, intent: intent) { return .applied }
        guard task.archived == intent.originalArchived && task.name == intent.taskName else {
            requireReview("The task changed on another client. Review its current state before continuing.", latest: task)
            return .review
        }
        guard intent.action != .archive || snapshot.active?.taskId != intent.taskID else {
            requireReview("Stop tracking this task before archiving it.", latest: task)
            return .review
        }
        return .apply
    }

    func responseMatches(_ snapshot: TrackerSnapshot, intent: Intent) -> Bool {
        snapshot.tasks.contains { $0.id == intent.taskID && $0.archived == intent.action.desiredArchived } &&
            (intent.action != .archive || snapshot.active?.taskId != intent.taskID)
    }

    func requireReview(_ message: String, latest: TaskItem?) {
        intent = nil
        pending = false
        isSubmitting = false
        isPresented = true
        requiresReview = true
        error = message
        self.latest = latest
    }

    func reviewLatest(_ task: TaskItem?, active: WorklogItem?) {
        guard requiresReview, let task, task.id == original?.id else { return }
        latest = task
        guard action != .archive || active?.taskId != task.id else {
            error = "Stop tracking this task before archiving it."
            return
        }
        original = task
        requiresReview = false
        error = nil
    }

    func setStatusVisibility(_ visible: Bool) {
        guard action == .unarchive, error != nil else { return }
        isPresented = visible
    }

    func fail(_ failure: Error, retainIntent: Bool) {
        isSubmitting = false
        pending = false
        isPresented = true
        error = failure.localizedDescription
        if !retainIntent { intent = nil }
    }

    func reset() {
        original = nil
        action = .archive
        isPresented = false
        isSubmitting = false
        error = nil
        requiresReview = false
        latest = nil
        intent = nil
        pending = false
    }
}
