import Foundation
import Observation

/// Holds the running worklog, the tracking error, the in-flight flag and the timer text for the whole app.
///
/// Views read the public properties; `TrackerSync` and `TrackerSession` write them. Each observed property is
/// assigned only when its value changes, so a display tick redraws the timer text only when the shown second
/// changes. The timer text and the error change only when `TrackerSync` publishes.
@Observable
public final class TrackingStore: ChangeGuardedStore {
    public private(set) var active: WorklogItem?
    public private(set) var error: String?
    public private(set) var isSubmitting = false
    /// The elapsed time of the running worklog, "Idle", or "Unavailable" before the tracker state is confirmed.
    public private(set) var timerText = "Unavailable"
    // The session records an error here when a request fails, and `publish` shows it at the next
    // publish, normally the one after the reload, as the presentation did.
    @ObservationIgnored private(set) var recordedError: String?
    // The elapsed time is measured from these with the monotonic uptime, so views don't observe them.
    @ObservationIgnored private var timerBase: TimeInterval = 0
    @ObservationIgnored private var timerAnchor: TimeInterval = 0

    func apply(_ worklog: WorklogItem?, clock: any TrackerClock) {
        let previous = active
        set(\.active, worklog)
        if previous?.id != active?.id || previous?.start != active?.start {
            resetElapsedAnchor(clock: clock)
        }
    }

    func resetElapsedAnchor(clock: any TrackerClock) {
        let now = clock.now
        timerBase = max(0, now.timeIntervalSince(parseTimestamp(active?.start) ?? now))
        timerAnchor = clock.uptime
    }

    func elapsed(clock: any TrackerClock) -> TimeInterval? {
        guard active != nil else { return nil }
        return timerBase + max(0, clock.uptime - timerAnchor)
    }

    func displayText(clock: any TrackerClock, confirmed: Bool) -> String {
        guard confirmed else { return "Unavailable" }
        return elapsed(clock: clock).map(formatClockDuration) ?? "Idle"
    }

    /// Shows the recorded error and the current timer text. The session calls this each time it publishes.
    func publish(clock: any TrackerClock, confirmed: Bool) {
        set(\.timerText, displayText(clock: clock, confirmed: confirmed))
        set(\.error, recordedError)
    }

    func beginSubmitting() { set(\.isSubmitting, true) }

    func endSubmitting() { set(\.isSubmitting, false) }

    func recordError(_ message: String) { recordedError = message }

    func clearError() { recordedError = nil }
}
