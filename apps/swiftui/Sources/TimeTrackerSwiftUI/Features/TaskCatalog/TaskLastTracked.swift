import Foundation
import TrackerClient

/// When the task last started, in the one relative form the sidebar and the task header share.
func lastTrackedText(_ latestStart: String?, now: Date = .now, calendar: Calendar = .current) -> String {
    guard let date = parseTimestamp(latestStart) else { return "Never tracked" }
    if calendar.isDate(date, inSameDayAs: now) {
        return "Last tracked today, \(date.formatted(date: .omitted, time: .shortened))"
    }
    let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: date), to: calendar.startOfDay(for: now))
    switch days.day ?? 0 {
    case 1: return "Last tracked yesterday"
    case 2..<7: return "Last tracked on \(date.formatted(.dateTime.weekday(.wide)))"
    default: return "Last tracked on \(date.formatted(.dateTime.day().month(.abbreviated)))"
    }
}
