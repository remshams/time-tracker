import SwiftUI
import TrackerClient

/// One worklog on one line: its times and its duration. Hovering highlights the matching segment on the day's strip,
/// and a double click opens the times for editing.
struct WorklogRow: View {
    let segment: DaySegment
    let color: Color
    let active: WorklogItem?
    @Binding var hovered: String?
    @Environment(WorklogCorrectionStore.self) private var correction

    private var worklog: WorklogItem { segment.worklog }
    private var isRunning: Bool { active?.id == worklog.id }

    var body: some View {
        HStack(spacing: HistoryColumns.spacing) {
            Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "now")")
                .monospacedDigit()
                .lineLimit(1)
                .fontWeight(isRunning ? .semibold : .regular)
                .foregroundStyle(isRunning ? color : Color.primary)
                .frame(width: HistoryColumns.range, alignment: .leading)
            Group {
                if isRunning {
                    RunningDuration()
                } else {
                    Text(readableDuration(segment.end.timeIntervalSince(segment.start)))
                }
            }
            .monospacedDigit()
            .fontWeight(isRunning ? .semibold : .regular)
            .foregroundStyle(isRunning ? color : Color.secondary)
            .frame(width: HistoryColumns.duration, alignment: .trailing)
            Spacer(minLength: 0)
            WorklogActionsMenu(worklog: worklog)
        }
        .frame(height: 28)
        .background {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(hovered == worklog.id ? AnyShapeStyle(.quaternary) : AnyShapeStyle(.clear))
                .padding(.horizontal, -8)
        }
        .contentShape(Rectangle())
        .onHover { hovered = $0 ? worklog.id : (hovered == worklog.id ? nil : hovered) }
        .onTapGesture(count: 2) {
            if correction.canOpen { correction.open(worklogID: worklog.id) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("worklog-history.row.\(worklog.id)")
        .contextMenu {
            WorklogContextMenu(worklog: worklog)
        }
    }
}

/// The running worklog's elapsed time. Only this view redraws on each tick.
private struct RunningDuration: View {
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        Text(readableDuration(clockText: tracking.timerText))
            .accessibilityLabel(tracking.timerText)
            .accessibilityIdentifier("worklog-history.running-duration")
    }
}

private struct WorklogActionsMenu: View {
    let worklog: WorklogItem
    @Environment(WorklogMoveStore.self) private var move
    @Environment(WorklogDeletionStore.self) private var deletion
    @Environment(WorklogCorrectionStore.self) private var correction

    var body: some View {
        Menu {
            WorklogContextMenu(worklog: worklog)
        } label: {
            Label("Worklog actions", systemImage: "ellipsis")
                .labelStyle(.iconOnly)
                .foregroundStyle(.tertiary)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .frame(width: HistoryColumns.actions, height: 22)
        .disabled(!correction.canOpen && !move.canOpen && !deletion.canOpen)
        .help("Worklog actions")
        .accessibilityIdentifier("worklog-history.actions.\(worklog.id)")
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
