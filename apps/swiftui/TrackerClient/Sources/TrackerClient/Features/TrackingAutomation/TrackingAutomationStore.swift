import Foundation
import Observation

/// Holds the screen-lock auto-pause preference, its status and its bookkeeping for the whole app, and pauses the
/// running timer when the screen locks and resumes its task when the screen unlocks.
///
/// Views read `enabled` and `statusText` and turn the auto-pause on or off; the lifecycle observer reports the screen
/// lock events. Each observed property is assigned only when its value changes. The status text changes only when
/// `TrackerSync` publishes.
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
    /// Reloads the task list after a pause or resume and publishes on every path. `TrackerFeatures` sets it to
    /// `TrackerSync`'s reload.
    @ObservationIgnored var reloadTaskList: @MainActor () async -> Void = {}
    /// Shows the status text, the tracking error and the other derived values. `TrackerFeatures` sets it to
    /// `TrackerSync`'s publish.
    @ObservationIgnored var publishChanges: @MainActor () -> Void = {}
    private let repository: any TrackingPreferencesRepository
    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let tracking: TrackingStore

    init(
        repository: any TrackingPreferencesRepository, client: any TrackerClient, clock: any TrackerClock,
        tracking: TrackingStore
    ) {
        self.repository = repository
        self.client = client
        self.clock = clock
        self.tracking = tracking
        enabled = repository.load().pauseOnScreenLock
    }

    /// Turns the screen-lock auto-pause on or off, saves the preference and publishes. Turning it on while the
    /// screen is locked pauses the running timer.
    public func setPauseOnScreenLock(_ enabled: Bool) {
        guard self.enabled != enabled else { return }
        set(\.enabled, enabled)
        cancel()
        repository.save(TrackingPreferences(pauseOnScreenLock: enabled))
        if enabled && locked { pauseTracking(at: clock.now) }
        publishChanges()
    }

    /// Pauses the running timer when the screen locks and resumes its task when the screen unlocks, if the
    /// auto-pause is on, and publishes. `date` is when the event happened, which the requests send as their time.
    public func setScreenLocked(_ locked: Bool, at date: Date) {
        if locked {
            guard lock() else { return }
            if enabled { pauseTracking(at: date) }
        } else if let resume = unlock(at: date) {
            Task { [weak self] in
                guard let self else { return }
                await resumeTracking(resume)
                await reloadTaskList()
            }
        }
        publishChanges()
    }

    /// The status text that the next publish shows.
    var pausedStatusText: String? {
        pausedTaskID == nil ? nil : "Paused while screen is locked"
    }

    /// Shows the current status text. `TrackerSync` calls this each time it publishes.
    func publish() { set(\.statusText, pausedStatusText) }

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

    /// Takes in the running worklog of a loaded task list. A running worklog means the user tracks again, so the
    /// auto-pause no longer resumes the task it stopped.
    func acceptTaskList(active: WorklogItem?) {
        if active != nil { pausedTaskID = nil }
    }

    /// Drops a pending auto-resume, which belongs to the previous connection.
    func acceptConnection() { cancel() }

    func cancel() {
        pausedTaskID = nil
        pauseMayResume = false
        unlockedAt = nil
    }

    private func pauseTracking(at date: Date) {
        guard let worklog = tracking.active, beginPause() else { return }
        tracking.clearError()
        let occurredAt = formatTimestamp(date)
        Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await client.pauseTracking(worklogID: worklog.id, occurredAt: occurredAt)
                if let resume = paused(taskID: worklog.taskId, didStop: result.didStop) {
                    await resumeTracking(resume)
                }
            } catch {
                isPausing = false
                tracking.recordFailure(error)
            }
            await reloadTaskList()
        }
    }

    /// Only follows a successful pause, which already cleared the tracking error.
    private func resumeTracking(_ resume: Resume) async {
        do {
            _ = try await client.resumeTracking(taskID: resume.taskID, occurredAt: formatTimestamp(resume.date))
        } catch {
            tracking.recordFailure(error)
        }
    }
}
