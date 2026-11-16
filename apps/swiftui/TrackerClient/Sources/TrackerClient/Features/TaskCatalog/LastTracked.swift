import Foundation

/// When a task last started, as a sidebar row's second line: "Today 13:12", "Yesterday", "3 days ago", a date such as
/// "4 Nov 2026", or "Never" for a task without worklogs. The text counts days from the start of `today`, so it changes
/// only when the day does.
public struct LastTracked: Equatable, Sendable {
    public let text: String

    public init(
        latestStart: String?, today: Date, calendar: Calendar, locale: Locale = .autoupdatingCurrent
    ) {
        guard let start = parseTimestamp(latestStart) else {
            text = "Never"
            return
        }
        let days = calendar.dateComponents(
            [.day], from: calendar.startOfDay(for: start), to: calendar.startOfDay(for: today)
        ).day
        switch days {
        case 0:
            let time = Date.FormatStyle(
                date: .omitted, time: .shortened, locale: locale, calendar: calendar, timeZone: calendar.timeZone)
            text = "Today \(start.formatted(time))"
        case 1:
            text = "Yesterday"
        case let days? where days > 1 && days < 7:
            text = "\(days) days ago"
        default:
            // A start after today, from another computer's clock, shows its date too.
            let date = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone)
                .day().month(.abbreviated).year()
            text = start.formatted(date)
        }
    }
}
