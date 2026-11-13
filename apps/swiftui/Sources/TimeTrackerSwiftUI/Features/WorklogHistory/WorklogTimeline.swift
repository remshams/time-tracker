import SwiftUI
import TrackerClient

/// The history's column grid. A row is the range, the worklog's bar on the day axis, the duration and the actions menu.
/// The bar takes the width that the fixed columns leave.
enum HistoryColumns {
    static let maxWidth: CGFloat = 720
    static let gutter: CGFloat = 24
    static let range: CGFloat = usesTwelveHourClock ? 150 : 112
    static let duration: CGFloat = 64
    static let actions: CGFloat = 26
    /// The duration and actions columns after a row's bar, with the spacing between them.
    static var trailing: CGFloat { spacing + duration + spacing + actions }
    static let spacing: CGFloat = 8

    private static var usesTwelveHourClock: Bool {
        (DateFormatter.dateFormat(fromTemplate: "j", options: 0, locale: .current) ?? "").contains("a")
    }
}

extension View {
    /// Centers the view in a column of the history's width.
    func readingColumn() -> some View {
        frame(maxWidth: HistoryColumns.maxWidth, alignment: .leading)
            .frame(maxWidth: .infinity)
    }
}

/// One worklog on the shared time axis, as a segment in a hairline track. The running worklog is hatched and ends at a
/// needle. Every row draws its own bar, so the row reads as one unit and the bars line up from one day to the next.
struct WorklogBar: View {
    let segment: DaySegment
    let day: WorklogDay
    let axis: TimeAxis
    let color: Color
    let highlighted: Bool

    private let trackHeight: CGFloat = 8

    var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            let from = axis.fraction(segment.start, on: day.date)
            let to = axis.fraction(segment.end, on: day.date)
            let segmentWidth = max(4, width * (to - from))
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 3, style: .continuous)
                    .fill(.quaternary.opacity(0.6))
                    .frame(height: trackHeight)
                ForEach(axis.ticks, id: \.self) { hour in
                    Rectangle()
                        .fill(.quaternary)
                        .frame(width: 1, height: trackHeight)
                        .offset(x: width * Double(hour - axis.startHour) / axis.span)
                }
                SegmentShape(color: color, hatched: segment.isRunning, highlighted: highlighted)
                    .frame(width: segmentWidth, height: trackHeight)
                    .offset(x: min(width * from, max(0, width - segmentWidth)))
                if segment.isRunning {
                    Capsule()
                        .fill(color)
                        .frame(width: 2, height: trackHeight + 6)
                        .offset(x: min(width - 2, width * to))
                }
            }
            .frame(maxHeight: .infinity)
        }
        .frame(height: 18)
        .accessibilityHidden(true)
    }
}

private struct SegmentShape: View {
    let color: Color
    let hatched: Bool
    let highlighted: Bool

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 3, style: .continuous)
        if hatched {
            shape.fill(color.opacity(0.28))
                .overlay {
                    Canvas { context, size in
                        var path = Path()
                        var x = -size.height
                        while x < size.width {
                            path.move(to: CGPoint(x: x, y: size.height))
                            path.addLine(to: CGPoint(x: x + size.height, y: 0))
                            x += 5
                        }
                        context.stroke(path, with: .color(color), lineWidth: 1.5)
                    }
                    .clipShape(shape)
                }
                .overlay { shape.strokeBorder(color, lineWidth: highlighted ? 1.5 : 1) }
        } else {
            shape.fill(color.opacity(highlighted ? 1 : 0.82))
        }
    }
}

/// Hour labels above the rows' bars. They span the bar column, so each label sits over its hairline, and the empty
/// ends match the range column and the duration and actions columns of a row.
struct TimeAxisLabels: View {
    let axis: TimeAxis

    var body: some View {
        HStack(spacing: HistoryColumns.spacing) {
            Color.clear.frame(width: HistoryColumns.range, height: 1)
            GeometryReader { proxy in
                // Full labels need about 36 points each. A narrow column keeps the bare hour.
                let narrow = proxy.size.width / CGFloat(axis.ticks.count) < 60
                ForEach(axis.ticks, id: \.self) { hour in
                    Text(narrow ? String(hour) : String(format: "%02d:00", hour))
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.tertiary)
                        .fixedSize()
                        .position(
                            x: proxy.size.width * Double(hour - axis.startHour) / axis.span, y: proxy.size.height / 2)
                }
            }
            Color.clear.frame(width: HistoryColumns.trailing, height: 1)
        }
        .frame(height: 14)
        .accessibilityHidden(true)
    }
}
