import Foundation

@MainActor
final class TrackingAutomationState {
    private(set) var enabled: Bool
    private(set) var locked = false
    private var pausedTaskID: String?

    init(preferences: TrackingPreferences) { enabled = preferences.pauseOnScreenLock }

    var statusText: String? {
        pausedTaskID == nil ? nil : "Paused while screen is locked"
    }

    func setEnabled(_ enabled: Bool) -> Bool {
        guard self.enabled != enabled else { return false }
        self.enabled = enabled
        pausedTaskID = nil
        return true
    }

    /// Returns false when the screen was already locked.
    func lock() -> Bool {
        guard !locked else { return false }
        locked = true
        return true
    }

    /// Remembers a task stopped by the screen-lock pause so that unlocking can resume it.
    func paused(taskID: String, didStop: Bool) {
        if didStop && enabled && locked { pausedTaskID = taskID }
    }

    /// Returns the task to resume, if the screen-lock pause stopped one.
    func unlock() -> String? {
        guard locked else { return nil }
        locked = false
        defer { pausedTaskID = nil }
        return enabled ? pausedTaskID : nil
    }

    func observeActive(_ active: WorklogItem?) {
        if active != nil { pausedTaskID = nil }
    }

    func cancel() {
        pausedTaskID = nil
    }
}
