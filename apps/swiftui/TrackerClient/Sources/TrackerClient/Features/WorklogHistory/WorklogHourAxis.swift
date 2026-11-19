import Foundation

/// The hours that every row's bar spans, so that bars line up from one day to the next.
///
/// The axis fits the loaded rows, rounded out to whole hours, and is at least `minimumSpan` hours wide. Without rows it
/// shows 9 to 17.
public struct WorklogHourAxis: Equatable, Sendable {
    static let minimumSpan = 8

    public let startHour: Int
    public let endHour: Int

    init(rows: [WorklogDayRow]) {
        var low = 9
        var high = 17
        if let first = rows.map(\.startHour).min(), let last = rows.map(\.endHour).max() {
            low = Int(first.rounded(.down))
            high = max(low, Int(last.rounded(.up)))
        }
        // Widen both ways, earlier first, until the axis is wide enough. A full day always is.
        while high - low < Self.minimumSpan {
            if low > 0 { low -= 1 }
            if high - low < Self.minimumSpan, high < 24 { high += 1 }
        }
        startHour = low
        endHour = high
    }

    /// Where `hours` sits on the axis, from 0 at its start to 1 at its end.
    public func position(_ hours: Double) -> Double {
        min(1, max(0, (hours - Double(startHour)) / Double(endHour - startHour)))
    }

    /// The hours between the ends that get a hairline: every second hour on a short axis, every third or fourth on a
    /// longer one.
    public var gridHours: [Int] {
        (startHour + 1..<endHour).filter { $0 % step == 0 }
    }

    /// The hours that get a label: both ends, and the hairlines at least a step away from them, so labels don't crowd.
    public var labeledHours: [Int] {
        let inner = gridHours.filter { $0 - startHour >= step && endHour - $0 >= step }
        return [startHour] + inner + [endHour]
    }

    private var step: Int {
        let span = endHour - startHour
        return span <= 10 ? 2 : span <= 15 ? 3 : 4
    }

    /// An hour's label: "9" and "17" on a 24-hour clock, "9am" and "5pm" on a 12-hour one. Midnight at the end of the
    /// day is "24" or "12am".
    public static func label(_ hour: Int, locale: Locale = .autoupdatingCurrent) -> String {
        let pattern = DateFormatter.dateFormat(fromTemplate: "j", options: 0, locale: locale) ?? ""
        guard pattern.contains("a") else { return String(hour) }
        let clock = hour % 12 == 0 ? 12 : hour % 12
        return "\(clock)\(hour % 24 < 12 ? "am" : "pm")"
    }
}
