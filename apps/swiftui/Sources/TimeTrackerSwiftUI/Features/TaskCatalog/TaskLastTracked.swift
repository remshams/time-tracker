import Foundation
import TrackerClient

/// When the task last started, as the sidebar row's short second line. Today shows the time, the days after it count
/// in whole days, and older starts show the date. No phrase mixes a fixed English word with a localized one.
func lastTrackedText(_ latestStart: String?, now: Date = .now, calendar: Calendar = .current) -> String {
    guard let date = parseTimestamp(latestStart) else { return "Never" }
    if calendar.isDate(date, inSameDayAs: now) {
        return "Today \(date.formatted(date: .omitted, time: .shortened))"
    }
    let days = calendar.dateComponents([.day], from: calendar.startOfDay(for: date), to: calendar.startOfDay(for: now))
    switch days.day ?? 0 {
    case 1: return "Yesterday"
    case 2..<7: return "\(days.day ?? 0) days ago"
    default: return date.formatted(.dateTime.day().month(.abbreviated).year(.defaultDigits))
    }
}
