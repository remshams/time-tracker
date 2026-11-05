import Foundation
import Observation

/// Holds the screen-lock auto-pause preference, its status and its bookkeeping for the whole app.
///
/// Views read `enabled` and `statusText`; `TrackerSync` writes them. Each observed property is assigned only
/// when its value changes. The status text changes only when `TrackerSync` publishes.
@Observable
public final class TrackingAutomationStore: ChangeGuardedStore {
    struct Resume {
        let taskID: String
        let date: Date
    }

    public private(set) var enabled: Bool
    /// "Paused while screen is locked" while the auto-pause holds a stopped task for the unlock, otherwise nil.
    public private(set) var statusText: String?
    // Only the auto-pause flow and the menu's primary action read these, so views don't observe them.
    @ObservationIgnored private(set) var locked = false
    @ObservationIgnored private var pausedTaskID: String?
    @ObservationIgnored private var isPausing = false
    @ObservationIgnored private var pauseMayResume = false
    @ObservationIgnored private var unlockedAt: Date?

    init(preferences: TrackingPreferences) { enabled = preferences.pauseOnScreenLock }

    /// The status text that the next publish shows.
    var pausedStatusText: String? {
        pausedTaskID == nil ? nil : "Paused while screen is locked"
    }

    /// Shows the current status text. `TrackerSync` calls this each time it publishes.
    func publish() { set(\.statusText, pausedStatusText) }

    func setEnabled(_ enabled: Bool) -> Bool {
        guard self.enabled != enabled else { return false }
        set(\.enabled, enabled)
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
