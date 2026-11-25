import SwiftUI
import TrackerClient

/// The history's column grid. A row is the range, the bar on the hour axis and the duration. The bar takes the width
/// that the fixed columns leave.
enum HistoryColumns {
    static let maxWidth: CGFloat = 720
    static let gutter = Spacing.xLarge
    static let range: CGFloat = usesTwelveHourClock ? 150 : 112
    static let duration: CGFloat = 64
    static let spacing = Spacing.small
    static let rowHeight: CGFloat = 28

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

/// A worklog's part of a day on the shared hour axis, as a segment in a hairline track. The running part is hatched
/// and ends at a "now" needle. Every row draws its own bar, so the row reads as one unit and the bars line up from one
/// day to the next. The dialogs that change a worklog show it with the same bar.
struct WorklogBar: View {
    /// Where the part starts and ends on its day, in wall-clock hours from 0 to 24.
    let startHour: Double
    let endHour: Double
    let isRunning: Bool
    let axis: WorklogHourAxis
    let color: Color
    let highlighted: Bool

    private let trackHeight: CGFloat = 8
    private let minimumWidth: CGFloat = 4
    /// How far the needle reaches above and below the track, together.
    private let needleOverhang: CGFloat = 6

    var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            let from = axis.position(startHour)
            let to = axis.position(endHour)
            let segmentWidth = max(minimumWidth, width * (to - from))
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: CornerRadius.small, style: .continuous)
                    .fill(.quinary)
                    .frame(height: trackHeight)
                ForEach(axis.gridHours, id: \.self) { hour in
                    Rectangle()
                        .fill(.quaternary)
                        .frame(width: BorderWidth.hairline, height: trackHeight)
                        .offset(x: width * axis.position(Double(hour)))
                }
                WorklogSegment(color: color, hatched: isRunning, highlighted: highlighted)
                    .frame(width: segmentWidth, height: trackHeight)
                    .offset(x: min(width * from, max(0, width - segmentWidth)))
                if isRunning {
                    Capsule()
                        .fill(color)
                        .frame(width: BorderWidth.needle, height: trackHeight + needleOverhang)
                        .offset(x: min(width - BorderWidth.needle, width * to))
                }
            }
            .frame(maxHeight: .infinity)
        }
        .frame(height: HistoryColumns.rowHeight)
        .accessibilityHidden(true)
    }
}

private struct WorklogSegment: View {
    let color: Color
    let hatched: Bool
    let highlighted: Bool

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: CornerRadius.small, style: .continuous)
        if hatched {
            shape.fill(color.opacity(Emphasis.runningFill))
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
                .overlay {
                    shape.strokeBorder(color, lineWidth: highlighted ? BorderWidth.marker : BorderWidth.hairline)
                }
        } else {
            shape.fill(color.opacity(highlighted ? 1 : Emphasis.restingBar))
        }
    }
}

/// The hour labels above the rows' bars. They span the bar column, so each label sits over its hairline.
struct WorklogHourLabels: View {
    let axis: WorklogHourAxis

    var body: some View {
        HStack(spacing: HistoryColumns.spacing) {
            Color.clear.frame(width: HistoryColumns.range, height: 1)
            WorklogHourScale(axis: axis)
            Color.clear.frame(width: HistoryColumns.duration, height: 1)
        }
    }
}

/// The axis's hour labels across the full width, each over its hairline. The first and last label mark the ends of
/// the axis and stay inside it. A narrow width keeps only the ends.
struct WorklogHourScale: View {
    let axis: WorklogHourAxis

    var body: some View {
        GeometryReader { proxy in
            let hours = axis.labeledHours
            // A bare hour needs about 20 points, "12pm" a few more.
            let shown = proxy.size.width / CGFloat(hours.count) < 22 ? [axis.startHour, axis.endHour] : hours
            ZStack {
                ForEach(shown, id: \.self) { hour in
                    let text = Text(WorklogHourAxis.label(hour))
                        .font(TextRole.duration(.caption2))
                        .foregroundStyle(.tertiary)
                        .fixedSize()
                    if hour == axis.startHour {
                        text.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                    } else if hour == axis.endHour {
                        text.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .trailing)
                    } else {
                        text.position(
                            x: proxy.size.width * axis.position(Double(hour)), y: proxy.size.height / 2)
                    }
                }
            }
        }
        .frame(height: 14)
        .accessibilityHidden(true)
    }
}
