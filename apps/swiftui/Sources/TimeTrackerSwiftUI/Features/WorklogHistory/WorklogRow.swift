import SwiftUI
import TrackerClient

/// One worklog's part of a day on one line: the worklog's times, the part's bar on the hour axis and its duration. A
/// double click opens the times for editing, and the context menu holds the worklog's actions. VoiceOver users reach
/// the same actions through the row's accessibility actions.
struct WorklogRow: View {
    let row: WorklogDayRow
    let axis: WorklogHourAxis
    let color: Color
    @Environment(WorklogCorrectionStore.self) private var correction
    @State private var isHovered = false

    var body: some View {
        HStack {
            HStack(spacing: HistoryColumns.spacing) {
                Text(row.range)
                    .monospacedDigit()
                    .lineLimit(1)
                    .fontWeight(row.isRunning ? .semibold : .regular)
                    .foregroundStyle(row.isRunning ? color : Color.primary)
                    .frame(width: HistoryColumns.range, alignment: .leading)
                WorklogBar(row: row, axis: axis, color: color, highlighted: isHovered || row.isRunning)
                Group {
                    if let duration = row.duration {
                        DurationLabel(duration: duration)
                    } else {
                        Text("–")
                    }
                }
                .font(TextRole.duration(.body))
                .fontWeight(row.isRunning ? .semibold : .regular)
                .foregroundStyle(row.isRunning ? color : Color.primary)
                .lineLimit(1)
                .frame(width: HistoryColumns.duration, alignment: .trailing)
            }
            // VoiceOver reads the times, the duration and whether it runs as one line.
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(row.accessibilityLabel)
            .accessibilityIdentifier("worklog-history.summary.\(row.id)")
            .accessibilityActions { WorklogContextMenu(worklog: row.worklog) }
        }
        .frame(minHeight: HistoryColumns.rowHeight)
        .background {
            RoundedRectangle(cornerRadius: CornerRadius.small, style: .continuous)
                .fill(isHovered ? AnyShapeStyle(.quinary) : AnyShapeStyle(.clear))
                .padding(.horizontal, -Spacing.small)
        }
        // No content shape: it would also become the row's accessibility shape, which breaks hit testing once the
        // history scrolls. The range, the bar's track and the duration cover the row for the double click.
        .onHover { isHovered = $0 }
        .onTapGesture(count: 2) {
            if correction.canOpen { correction.open(worklogID: row.worklog.id) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("worklog-history.row.\(row.id)")
        .contextMenu {
            WorklogContextMenu(worklog: row.worklog)
        }
    }
}

private struct WorklogContextMenu: View {
    let worklog: WorklogItem
    @Environment(WorklogMoveStore.self) private var move
    @Environment(WorklogDeletionStore.self) private var deletion
    @Environment(WorklogCorrectionStore.self) private var correction

    var body: some View {
        Button("Edit times...") { correction.open(worklogID: worklog.id) }
            .disabled(!correction.canOpen)
        Button("Move to task...") { move.open(worklogID: worklog.id) }
            .disabled(!move.canOpen)
        Divider()
        // Running worklogs cannot be deleted. Stop the timer first.
        Button("Delete...", role: .destructive) { deletion.open(worklogID: worklog.id) }
            .disabled(!deletion.canOpen || worklog.end == nil)
    }
}
