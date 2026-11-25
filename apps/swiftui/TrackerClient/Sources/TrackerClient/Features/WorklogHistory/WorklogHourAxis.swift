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
        self.init(ranges: rows.map { ($0.startHour, $0.endHour) })
    }

    /// An axis that fits every span, for a dialog that shows one worklog. Passing the span the dialog opened with
    /// next to the edited one keeps the scale still, so an edit moves the bar instead.
    public init(covering spans: [WorklogSpan]) {
        self.init(ranges: spans.map { ($0.startHour, $0.endHour) })
    }

    private init(ranges: [(start: Double, end: Double)]) {
        var low = 9
        var high = 17
        if let first = ranges.map(\.start).min(), let last = ranges.map(\.end).max() {
            low = Int(first.rounded(.down))
            high = max(low, Int(last.rounded(.up)))
        }
        // Widen both ways, the start by the larger half, then give what midnight blocks on one side to the other.
        let missing = max(0, Self.minimumSpan - (high - low))
        let earlier = min(low, (missing + 1) / 2)
        high = min(24, high + missing - earlier)
        low = min(low - earlier, high - Self.minimumSpan)
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
