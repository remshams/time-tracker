import Foundation

final class TrackingState {
    private(set) var active: WorklogItem?
    var error: String?
    var isSubmitting = false
    private var timerBase: TimeInterval = 0
    private var timerAnchor: TimeInterval = 0

    func apply(_ worklog: WorklogItem?, clock: any TrackerClock) {
        let previous = active
        if active != worklog { active = worklog }
        if previous?.id != active?.id || previous?.start != active?.start {
            resetElapsedAnchor(clock: clock)
        }
    }

    func resetElapsedAnchor(clock: any TrackerClock) {
        let now = clock.now
        timerBase = max(0, now.timeIntervalSince(timestamp(active?.start) ?? now))
        timerAnchor = clock.uptime
    }

    func elapsed(clock: any TrackerClock) -> TimeInterval? {
        guard active != nil else { return nil }
        return timerBase + max(0, clock.uptime - timerAnchor)
    }
}
