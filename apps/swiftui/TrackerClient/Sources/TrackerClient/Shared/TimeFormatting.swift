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

public func formatClockDuration(_ seconds: TimeInterval) -> String {
    let total = Int(max(0, seconds))
    return String(format: "%02d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
}
