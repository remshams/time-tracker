import Foundation

/// The window's one duration format: hours without padding, then padded minutes and seconds, as in `3:36:00`.
func readableDuration(_ seconds: TimeInterval) -> String {
    let total = Int(max(0, seconds))
    return String(format: "%d:%02d:%02d", total / 3600, total / 60 % 60, total % 60)
}

/// Converts a store's `HH:MM:SS` text to the window's duration format. Text in another shape passes through.
func readableDuration(clockText text: String) -> String {
    let parts = text.split(separator: ":").compactMap { Int($0) }
    guard parts.count == 3 else { return text }
    return readableDuration(TimeInterval(parts[0] * 3600 + parts[1] * 60 + parts[2]))
}

/// The seconds in a store's `HH:MM:SS` text, or `nil` for other text such as "Unavailable".
func clockSeconds(_ text: String) -> TimeInterval? {
    let parts = text.split(separator: ":").compactMap { Int($0) }
    guard parts.count == 3 else { return nil }
    return TimeInterval(parts[0] * 3600 + parts[1] * 60 + parts[2])
}
