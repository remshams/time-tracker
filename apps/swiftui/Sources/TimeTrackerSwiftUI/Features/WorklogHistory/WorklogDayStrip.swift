import SwiftUI
import TrackerClient

/// The history's column grid. The column holds the axis, the day strips and the rows. Within a row, the range and the
/// duration are fixed columns that sit next to each other, and the actions menu stays at the right edge.
enum HistoryColumns {
    static let maxWidth: CGFloat = 720
    static let gutter: CGFloat = 24
    static let range: CGFloat = usesTwelveHourClock ? 150 : 112
    static let duration: CGFloat = 64
    static let actions: CGFloat = 26
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

/// One day on the time axis: a segment for each worklog in the task color. The running worklog is hatched and ends
/// at a needle. Hovering a segment highlights its row, and clicking it opens its times for editing.
struct WorklogDayStrip: View {
    let day: WorklogDay
    let axis: TimeAxis
    let color: Color
    @Binding var hovered: String?
    @Environment(WorklogCorrectionStore.self) private var correction

    private let trackHeight: CGFloat = 12

    var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
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
                ForEach(day.segments) { segment in
                    let from = axis.fraction(segment.start, on: day.date)
                    let to = axis.fraction(segment.end, on: day.date)
                    let x = width * from
                    let segmentWidth = max(4, width * (to - from))
                    let highlighted = hovered == segment.id
                    SegmentShape(color: color, hatched: segment.isRunning, highlighted: highlighted)
                        .frame(width: segmentWidth, height: highlighted ? trackHeight + 4 : trackHeight)
                        .offset(x: min(x, max(0, width - segmentWidth)))
                        .animation(.easeOut(duration: 0.12), value: highlighted)
                }
                if day.isToday, let running = day.segments.first(where: \.isRunning) {
                    Capsule()
                        .fill(color)
                        .frame(width: 2, height: trackHeight + 8)
                        .offset(x: min(width - 2, width * axis.fraction(running.end, on: day.date)))
                }
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .onContinuousHover { phase in
                switch phase {
                case .active(let point):
                    hovered = segment(at: point.x, width: width)?.id
                case .ended:
                    hovered = nil
                }
            }
            .onTapGesture { location in
                if let segment = segment(at: location.x, width: width), correction.canOpen {
                    correction.open(worklogID: segment.id)
                }
            }
        }
        .frame(height: 24)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "\(day.title), \(day.segments.count) \(day.segments.count == 1 ? "worklog" : "worklogs") on the day's time line"
        )
    }

    /// The segment under a horizontal position, with a few points of slack around thin segments.
    private func segment(at x: CGFloat, width: CGFloat) -> DaySegment? {
        day.segments.min { distance($0, x, width) < distance($1, x, width) }
            .flatMap { distance($0, x, width) <= 5 ? $0 : nil }
    }

    private func distance(_ segment: DaySegment, _ x: CGFloat, _ width: CGFloat) -> CGFloat {
        let from = width * axis.fraction(segment.start, on: day.date)
        let to = max(from + 4, width * axis.fraction(segment.end, on: day.date))
        return x < from ? from - x : x > to ? x - to : 0
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

/// Hour labels above the day strips. They span the strips' width, so each label sits over its hairline.
struct TimeAxisLabels: View {
    let axis: TimeAxis

    var body: some View {
        GeometryReader { proxy in
            ForEach(axis.ticks, id: \.self) { hour in
                Text(String(format: "%02d:00", hour))
                    .font(.caption2.monospacedDigit())
                    .foregroundStyle(.tertiary)
                    .position(x: proxy.size.width * Double(hour - axis.startHour) / axis.span, y: proxy.size.height / 2)
            }
        }
        .frame(height: 14)
        .accessibilityHidden(true)
    }
}
