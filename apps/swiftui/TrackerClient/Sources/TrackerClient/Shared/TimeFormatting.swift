import Foundation

// Default main-actor isolation makes this formatter main-actor only, so its mutable state needs no lock.
private let isoFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
}()

public func parseTimestamp(_ value: String?) -> Date? {
    guard let value else { return nil }
    return isoFormatter.date(from: value)
}

func formatTimestamp(_ date: Date) -> String {
    isoFormatter.string(from: date)
}

/// A duration in whole seconds, never below zero, as the app shows it.
func wholeSeconds(_ duration: TimeInterval) -> Int {
    Int(max(0, duration))
}

/// A duration as `H:MM:SS` without a leading zero on the hour, split so a view can dim the seconds.
public struct DurationText: Equatable, Sendable {
    /// The hours and minutes, such as "1:04".
    public let hoursAndMinutes: String
    /// The seconds with their separator, such as ":27".
    public let seconds: String

    public init(seconds: Int) {
        let total = max(0, seconds)
        hoursAndMinutes = String(format: "%d:%02d", total / 3600, total / 60 % 60)
        self.seconds = String(format: ":%02d", total % 60)
    }

    /// The whole seconds of `duration`, never below zero.
    public init(duration: TimeInterval) {
        self.init(seconds: wholeSeconds(duration))
    }
}

/// A duration for VoiceOver in hours and minutes, such as "1 hour, 4 minutes". It reads seconds only under a minute.
public func spokenDuration(_ seconds: Int) -> String {
    let total = max(0, seconds)
    guard total >= 60 else { return counted(total, "second") }
    let hours = total / 3600
    let minutes = total / 60 % 60
    let parts = [hours > 0 ? counted(hours, "hour") : nil, minutes > 0 ? counted(minutes, "minute") : nil]
    return parts.compactMap { $0 }.joined(separator: ", ")
}

private func counted(_ value: Int, _ unit: String) -> String {
    "\(value) \(unit)\(value == 1 ? "" : "s")"
}
