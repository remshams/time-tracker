import Foundation

@MainActor
final class TrackingAutomationState {
    struct Pause {
        let occurredAt: Date
        let expectedWorklogID: String?
        let ownStartTaskID: String?
        let discoverAtStartup: Bool
    }

    private(set) var enabled: Bool
    private(set) var locked = false
    private(set) var generation = 0
    var pendingPause: Pause?
    var resumeAt: Date?
    var pausedTaskID: String?

    init(preferences: TrackingPreferences) { enabled = preferences.pauseOnScreenLock }

    var statusText: String? {
        pausedTaskID == nil ? nil : "Paused while screen is locked"
    }

    func setEnabled(
        _ enabled: Bool, at date: Date, active: WorklogItem?,
        ownStartTaskID: String?, confirmed: Bool
    ) -> Bool {
        guard self.enabled != enabled else { return false }
        self.enabled = enabled
        cancel()
        if enabled && locked && (active != nil || ownStartTaskID != nil || !confirmed) {
            pendingPause = Pause(
                occurredAt: date, expectedWorklogID: active?.id,
                ownStartTaskID: ownStartTaskID, discoverAtStartup: !confirmed)
        }
        return true
    }

    func lock(at date: Date, active: WorklogItem?, ownStartTaskID: String?, confirmed: Bool) {
        guard !locked else { return }
        locked = true
        resumeAt = nil
        if enabled && (active != nil || ownStartTaskID != nil || !confirmed) {
            pendingPause = Pause(
                occurredAt: date, expectedWorklogID: active?.id,
                ownStartTaskID: ownStartTaskID, discoverAtStartup: !confirmed)
        }
    }

    func observeActive(_ active: WorklogItem?) {
        if pausedTaskID != nil && active != nil {
            pausedTaskID = nil
            resumeAt = nil
        }
    }

    func acknowledgeOwnStart(_ active: WorklogItem?, taskID: String) {
        guard let pending = pendingPause, pending.ownStartTaskID == taskID,
            let active, active.taskId == taskID
        else { return }
        pendingPause = Pause(
            occurredAt: pending.occurredAt, expectedWorklogID: active.id,
            ownStartTaskID: nil, discoverAtStartup: false)
    }

    func unlock(at date: Date) {
        guard locked else { return }
        locked = false
        if enabled { resumeAt = date }
    }

    // Ownership is granted only by an acknowledged stop. In-flight replies cannot
    // restore ownership after disabling, changing connections, or shutdown.
    func cancel() {
        generation += 1
        pendingPause = nil
        resumeAt = nil
        pausedTaskID = nil
    }
}
