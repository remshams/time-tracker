import Foundation

/// The selected task's loaded worklogs grouped by local calendar day, newest day first, with one hour axis for all
/// days so that bars line up from one day to the next.
///
/// A worklog that crosses midnight is split into one row per day it touches. Each row covers the part on its day, so a
/// day's total is the sum of its rows. Today's total comes from the daily totals while they are available, so it
/// matches the sidebar and the task header.
public struct WorklogDays: Equatable, Sendable {
    public let days: [WorklogDay]
    public let axis: WorklogHourAxis

    /// Groups `worklogs` by the local days of `calendar`. The running worklog ends at `runningEnd`, or at its start
    /// while its elapsed time is unknown. `hasOlder` hides the oldest day's total, which would be partial;
    /// `todaySeconds` is today's total from the daily totals, or `nil` while they are unavailable.
    init(
        worklogs: [WorklogItem], runningEnd: Date?, hasOlder: Bool, today: Date, todaySeconds: Int?,
        calendar: Calendar, locale: Locale = .autoupdatingCurrent
    ) {
        let format = WorklogDayFormat(today: today, calendar: calendar, locale: locale)
        let parts = worklogs.flatMap { WorklogPart.parts(of: $0, runningEnd: runningEnd, calendar: calendar) }
        let byDay = Dictionary(grouping: parts.enumerated()) { $0.element.day }
        let oldest = byDay.keys.min()
        days = byDay.keys.sorted(by: >).map { day in
            // Newest first; the history's order breaks ties.
            let rows = byDay[day, default: []]
                .sorted { ($0.element.start, $1.offset) > ($1.element.start, $0.offset) }
                .map { WorklogDayRow(part: $0.element, format: format) }
            let sum = rows.reduce(0) { $0 + ($1.seconds ?? 0) }
            let total: Int?
            if format.isToday(day) {
                total = todaySeconds ?? (hasOlder && day == oldest ? nil : sum)
            } else {
                total = hasOlder && day == oldest ? nil : sum
            }
            return WorklogDay(id: day, title: format.title(day), totalSeconds: total, rows: rows)
        }
        axis = WorklogHourAxis(rows: days.flatMap(\.rows))
    }
}

extension WorklogDays {
    /// The selected task's days as the history panel shows them. Reading this from a view body observes the loaded
    /// worklogs, the running worklog's elapsed seconds, the current day and the task's daily total, so the view redraws
    /// when any of them changes.
    public init(
        history: WorklogHistoryStore, tracking: TrackingStore, dailyTotals: DailyTotalsStore, taskID: String
    ) {
        let runningEnd = tracking.active.flatMap { active in
            parseTimestamp(active.start).flatMap { start in
                tracking.elapsedSeconds.map { start.addingTimeInterval(Double($0)) }
            }
        }
        let isRunning = tracking.active?.taskId == taskID
        self.init(
            worklogs: history.worklogs, runningEnd: runningEnd, hasOlder: history.hasMore, today: dailyTotals.today(),
            todaySeconds: dailyTotals.taskTotal(taskID: taskID).content?.shownSeconds(isRunning: isRunning),
            calendar: dailyTotals.calendar)
    }
}

/// One local day of the history: its title, its total and its rows, newest first.
public struct WorklogDay: Identifiable, Equatable, Sendable {
    /// The day's local midnight.
    public let id: Date
    /// "Today", "Yesterday", or the weekday and date, such as "Tue 17 Nov".
    public let title: String
    /// The day's total in whole seconds, or `nil` for the oldest loaded day while older worklogs are unloaded.
    public let totalSeconds: Int?
    public let rows: [WorklogDayRow]

    /// The day's total as the window shows it, or `nil` when it is hidden.
    public var total: DurationText? { totalSeconds.map(DurationText.init(seconds:)) }

    /// The day header for VoiceOver, such as "Today, 1 hour, 4 minutes".
    public var accessibilityLabel: String {
        totalSeconds.map { "\(title), \(spokenDuration($0))" } ?? title
    }
}

/// One worklog's part on one day.
public struct WorklogDayRow: Identifiable, Equatable, Sendable {
    /// The worklog's ID for the part on its start day. Later parts of a worklog that crosses midnight add their day,
    /// such as "w1.2026-11-18", so that every row has its own ID.
    public let id: String
    public let worklog: WorklogItem
    /// The worklog's whole range, such as "09:12 – 10:40" or "23:10 – now", on every part of the worklog.
    public let range: String
    /// The part's duration in whole seconds, or `nil` for a running worklog whose elapsed time is unknown.
    public let seconds: Int?
    /// Whether this part ends now. Earlier parts of a running worklog have ended at midnight.
    public let isRunning: Bool
    /// Where the part starts and ends on its day, in wall-clock hours from 0 to 24.
    public let startHour: Double
    public let endHour: Double
    /// The row for VoiceOver, such as "09:12 to 10:40, 1 hour, 28 minutes".
    public let accessibilityLabel: String

    /// The part's duration as the window shows it, or `nil` while it is unknown.
    public var duration: DurationText? { seconds.map(DurationText.init(seconds:)) }

    fileprivate init(part: WorklogPart, format: WorklogDayFormat) {
        id = part.isFirst ? part.worklog.id : "\(part.worklog.id).\(format.key(part.day))"
        worklog = part.worklog
        let start = format.time(part.worklogStart)
        let end = part.worklog.end == nil ? "now" : format.time(part.worklogEnd)
        range = "\(start) – \(end)"
        seconds = part.seconds
        isRunning = part.isRunning
        startHour = format.hours(part.start, day: part.day)
        endHour = max(startHour, format.hours(part.end, day: part.day))
        var label = "\(start) to \(end)"
        if let seconds { label += ", \(spokenDuration(seconds))" }
        if isRunning { label += ", running" }
        accessibilityLabel = label
    }
}

/// A worklog's part on one local day.
private struct WorklogPart {
    let worklog: WorklogItem
    let worklogStart: Date
    let worklogEnd: Date
    let day: Date
    let start: Date
    let end: Date
    let isFirst: Bool
    let isRunning: Bool
    let seconds: Int?

    /// Splits a worklog at each local midnight. A worklog without a valid start has no parts. A finished worklog
    /// whose end can't be read, or ends before it starts, is a moment at its start.
    static func parts(of worklog: WorklogItem, runningEnd: Date?, calendar: Calendar) -> [WorklogPart] {
        guard let start = parseTimestamp(worklog.start) else { return [] }
        let running = worklog.end == nil
        let end = max(start, (running ? runningEnd : parseTimestamp(worklog.end)) ?? start)
        let known = !running || runningEnd != nil
        var parts: [WorklogPart] = []
        var day = calendar.startOfDay(for: start)
        repeat {
            guard let next = calendar.date(byAdding: .day, value: 1, to: day) else { break }
            let partStart = max(start, day)
            let partEnd = min(end, next)
            parts.append(
                WorklogPart(
                    worklog: worklog, worklogStart: start, worklogEnd: end, day: day, start: partStart, end: partEnd,
                    isFirst: parts.isEmpty, isRunning: running && partEnd == end,
                    seconds: known ? wholeSeconds(partEnd.timeIntervalSince(partStart)) : nil))
            day = next
        } while day < end
        return parts
    }
}

/// Names days and times in the user's calendar, time zone and locale.
private struct WorklogDayFormat {
    let today: Date
    let calendar: Calendar
    let locale: Locale

    init(today: Date, calendar: Calendar, locale: Locale) {
        self.today = calendar.startOfDay(for: today)
        self.calendar = calendar
        self.locale = locale
    }

    func isToday(_ day: Date) -> Bool { day == today }

    func title(_ day: Date) -> String {
        switch calendar.dateComponents([.day], from: day, to: today).day {
        case 0: return "Today"
        case 1: return "Yesterday"
        default:
            var style = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone)
                .weekday(.abbreviated).day().month(.abbreviated)
            if calendar.component(.year, from: day) != calendar.component(.year, from: today) {
                style = style.year()
            }
            return day.formatted(style)
        }
    }

    /// A day as "2026-11-18", for row IDs.
    func key(_ day: Date) -> String {
        let parts = calendar.dateComponents([.year, .month, .day], from: day)
        return String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
    }

    func time(_ date: Date) -> String {
        date.formatted(
            Date.FormatStyle(
                date: .omitted, time: .shortened, locale: locale, calendar: calendar,
                timeZone: calendar.timeZone))
    }

    /// The wall-clock hours of `date` on `day`, so that a day with a daylight saving change keeps its bars at their
    /// clock times. A date on the next day is 24.
    func hours(_ date: Date, day: Date) -> Double {
        guard calendar.isDate(date, inSameDayAs: day) else { return date > day ? 24 : 0 }
        let parts = calendar.dateComponents([.hour, .minute, .second, .nanosecond], from: date)
        let seconds = Double(parts.second ?? 0) + Double(parts.nanosecond ?? 0) / 1_000_000_000
        return Double(parts.hour ?? 0) + Double(parts.minute ?? 0) / 60 + seconds / 3600
    }
}
