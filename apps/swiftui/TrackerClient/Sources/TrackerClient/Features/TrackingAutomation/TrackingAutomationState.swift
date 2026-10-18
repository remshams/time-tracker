import Foundation

@MainActor
final class TrackingAutomationState {
    struct Resume {
        let taskID: String
        let date: Date
    }

    private(set) var enabled: Bool
    private(set) var locked = false
    private var pausedTaskID: String?
    private var isPausing = false
    private var pauseMayResume = false
    private var unlockedAt: Date?

    init(preferences: TrackingPreferences) { enabled = preferences.pauseOnScreenLock }

    var statusText: String? {
        pausedTaskID == nil ? nil : "Paused while screen is locked"
    }

    func setEnabled(_ enabled: Bool) -> Bool {
        guard self.enabled != enabled else { return false }
        self.enabled = enabled
        cancel()
        return true
    }

    /// Returns false when the screen was already locked.
    func lock() -> Bool {
        guard !locked else { return false }
        locked = true
        unlockedAt = nil
        return true
    }

    func beginPause() -> Bool {
        guard !isPausing else { return false }
        isPausing = true
        pauseMayResume = true
        return true
    }

    /// Remembers a task stopped by the screen-lock pause so that unlocking can resume it.
    func paused(taskID: String, didStop: Bool) -> Resume? {
        isPausing = false
        defer {
            pauseMayResume = false
            unlockedAt = nil
        }
        guard pauseMayResume, didStop, enabled else { return nil }
        if locked {
            pausedTaskID = taskID
            return nil
        }
        return unlockedAt.map { Resume(taskID: taskID, date: $0) }
    }

    func pauseFailed() {
        isPausing = false
    }

    /// Returns the task to resume, if the screen-lock pause stopped one.
    func unlock(at date: Date) -> Resume? {
        guard locked else { return nil }
        locked = false
        defer { pausedTaskID = nil }
        guard enabled else { return nil }
        if let pausedTaskID { return Resume(taskID: pausedTaskID, date: date) }
        if pauseMayResume { unlockedAt = date }
        return nil
    }

    func observeActive(_ active: WorklogItem?) {
        if active != nil { pausedTaskID = nil }
    }

    func cancel() {
        pausedTaskID = nil
        pauseMayResume = false
        unlockedAt = nil
    }
}
