import Foundation
import TrackerClient

/// One worklog placed on its day, as seconds since local midnight so that days share one time axis.
struct DaySegment: Identifiable {
    let worklog: WorklogItem
    let start: Date
    let end: Date
    let isRunning: Bool
    var id: String { worklog.id }
}

/// The selected task's worklogs of one local day, newest first.
struct WorklogDay: Identifiable {
    let date: Date
    let segments: [DaySegment]
    var id: Date { date }

    var isToday: Bool { Calendar.current.isDateInToday(date) }

    var title: String {
        let calendar = Calendar.current
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInYesterday(date) { return "Yesterday" }
        return date.formatted(.dateTime.weekday(.abbreviated).day().month(.abbreviated))
    }

    var seconds: TimeInterval { segments.reduce(0) { $0 + $1.end.timeIntervalSince($1.start) } }

    /// Groups worklogs by the day they start. A running worklog ends at `runningEnd`.
    static func grouping(_ worklogs: [WorklogItem], runningEnd: Date?) -> [WorklogDay] {
        let calendar = Calendar.current
        let segments = worklogs.compactMap { worklog -> DaySegment? in
            guard let start = parseTimestamp(worklog.start) else { return nil }
            let running = worklog.end == nil
            let end = parseTimestamp(worklog.end) ?? max(start, runningEnd ?? start)
            return DaySegment(worklog: worklog, start: start, end: max(start, end), isRunning: running)
        }
        let byDay = Dictionary(grouping: segments) { calendar.startOfDay(for: $0.start) }
        return byDay.keys.sorted(by: >).map { day in
            WorklogDay(date: day, segments: byDay[day, default: []].sorted { $0.start > $1.start })
        }
    }
}

/// The hours that every day's strip shows, so that strips line up from one day to the next.
struct TimeAxis: Equatable {
    let startHour: Int
    let endHour: Int

    var span: Double { Double(endHour - startHour) }

    /// Hours that get a label and a hairline.
    var ticks: [Int] {
        let step = span <= 10 ? 2 : span <= 15 ? 3 : 4
        return (startHour...endHour).filter { $0 % step == 0 && $0 > startHour && $0 < endHour }
    }

    /// Fits the hours worked, rounded out to whole hours, and never narrower than eight hours.
    init(days: [WorklogDay]) {
        var first = 24.0
        var last = 0.0
        for day in days {
            let midnight = day.date
            for segment in day.segments {
                first = min(first, segment.start.timeIntervalSince(midnight) / 3600)
                last = max(last, min(24, segment.end.timeIntervalSince(midnight) / 3600))
            }
        }
        if first >= last { (first, last) = (9, 17) }
        var low = Int(first.rounded(.down))
        var high = Int(last.rounded(.up))
        while high - low < 8 {
            if low > 0 { low -= 1 }
            if high - low < 8, high < 24 { high += 1 }
        }
        startHour = low
        endHour = high
    }

    func fraction(_ date: Date, on day: Date) -> Double {
        let hours = date.timeIntervalSince(day) / 3600
        return min(1, max(0, (hours - Double(startHour)) / span))
    }
}
