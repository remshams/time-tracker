import Foundation

@MainActor
final class TrackingState {
    private(set) var active: WorklogItem?
    var error: String?
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

private let isoFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
}()
private let isoFormatterLock = NSLock()

public func timestamp(_ value: String?) -> Date? {
    guard let value else { return nil }
    isoFormatterLock.lock()
    defer { isoFormatterLock.unlock() }
    return isoFormatter.date(from: value)
}

func commandTimestamp(_ date: Date) -> String {
    isoFormatterLock.lock()
    defer { isoFormatterLock.unlock() }
    return isoFormatter.string(from: date)
}

public func clockDuration(_ seconds: TimeInterval) -> String {
    let total = Int(max(0, seconds))
    return String(format: "%02d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
}
