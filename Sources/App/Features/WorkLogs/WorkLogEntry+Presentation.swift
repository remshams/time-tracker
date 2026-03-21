import Foundation

extension WorkLogEntry {
    var formattedTimeRange: String {
        let start = startedAt.formatted(date: .omitted, time: .shortened)
        if let endedAt {
            let end = endedAt.formatted(date: .omitted, time: .shortened)
            return "\(start) – \(end)"
        }
        let running = String(localized: "work-log-entry.running", defaultValue: "Running")
        return "\(start) – \(running)"
    }

    var formattedDuration: String {
        guard let duration else {
            return String(localized: "work-log-entry.duration.unknown", defaultValue: "–")
        }
        let totalSeconds = Int(duration.components.seconds)
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        if minutes > 0 {
            return "\(minutes)m"
        }
        return "\(seconds)s"
    }
}
