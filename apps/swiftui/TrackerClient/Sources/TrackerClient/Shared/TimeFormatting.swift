import Foundation

private let isoFormatter: ISO8601DateFormatter = {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter
}()
private let isoFormatterLock = NSLock()

public func timestamp(_ value: String?) -> Date? {
    guard let value else { return nil }
    return isoFormatterLock.withLock { isoFormatter.date(from: value) }
}

func commandTimestamp(_ date: Date) -> String {
    isoFormatterLock.withLock { isoFormatter.string(from: date) }
}

public func clockDuration(_ seconds: TimeInterval) -> String {
    let total = Int(max(0, seconds))
    return String(format: "%02d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
}
