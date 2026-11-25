import Foundation

/// One worklog the way a dialog shows it: its start day, its time range, and where it sits on that day's hours.
///
/// The bar covers the start day only. A worklog that ends on a later day reaches midnight on the bar, and its range
/// names the end day. An end before the start is a moment at the start.
public struct WorklogSpan: Equatable, Sendable {
    /// The start day, such as "Wednesday 18 November", with the year when it isn't the current one.
    public let day: String
    /// "9:12 – 10:40", "23:10 – Thu 19 Nov, 1:20" when it ends on another day, or "9:12 – now" while it runs.
    public let range: String
    /// Where the worklog starts and ends on its start day, in wall-clock hours from 0 to 24.
    public let startHour: Double
    public let endHour: Double

    /// A running worklog, with no `end`, reaches `now`.
    public init(start: Date, end: Date?, now: Date, calendar: Calendar, locale: Locale = .autoupdatingCurrent) {
        let format = WorklogDayFormat(today: now, calendar: calendar, locale: locale)
        let startDay = calendar.startOfDay(for: start)
        var dayStyle = format.style.weekday(.wide).day().month(.wide)
        if calendar.component(.year, from: start) != calendar.component(.year, from: now) {
            dayStyle = dayStyle.year()
        }
        day = start.formatted(dayStyle)
        if let end {
            let endDay = end.formatted(format.style.weekday(.abbreviated).day().month(.abbreviated))
            let endText = calendar.isDate(end, inSameDayAs: start) ? format.time(end) : "\(endDay), \(format.time(end))"
            range = "\(format.time(start)) – \(endText)"
        } else {
            range = "\(format.time(start)) – now"
        }
        startHour = format.hours(start, day: startDay)
        endHour = max(startHour, format.hours(end ?? now, day: startDay))
    }
}
