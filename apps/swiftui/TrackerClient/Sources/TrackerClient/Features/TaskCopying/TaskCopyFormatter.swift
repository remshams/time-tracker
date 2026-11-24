import Foundation

/// Writes a task's time today as the text a copy action copies.
public enum TaskCopyFormatter {
    public static func exact(_ duration: TimeInterval?) -> String? {
        guard let seconds = seconds(duration) else { return nil }
        let hours = seconds / 3_600
        let minutes = seconds % 3_600 / 60
        let remainder = seconds % 60
        if hours > 0 { return "\(hours)h \(minutes)m \(remainder)s" }
        if minutes > 0 { return "\(minutes)m \(remainder)s" }
        return "\(remainder)s"
    }

    public static func rounded(_ duration: TimeInterval?) -> String? {
        guard let duration, duration.isFinite else { return nil }
        let rounded = (max(0, duration) / 900).rounded(.toNearestOrAwayFromZero) * 900
        guard let seconds = seconds(rounded) else { return nil }
        let hours = seconds / 3_600
        let minutes = seconds % 3_600 / 60
        return hours > 0 ? "\(hours)h \(minutes)m" : "\(minutes)m"
    }

    private static func seconds(_ duration: TimeInterval?) -> Int64? {
        guard let duration, duration.isFinite, duration < Double(Int64.max) else { return nil }
        return Int64(max(0, duration))
    }
}
