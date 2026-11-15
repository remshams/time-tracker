import SwiftUI
import TrackerClient

struct WorklogRow: View {
    let worklog: WorklogItem
    let active: WorklogItem?

    private var duration: TimeInterval { worklogDuration(worklog) }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.medium) {
            VStack(alignment: .leading, spacing: Spacing.xSmall) {
                Text(localDay(worklog.start))
                    .font(TextRole.detailEmphasis)
                    .foregroundStyle(.primary)
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: Spacing.medium)
            Group {
                if active?.id == worklog.id {
                    TrackerElapsedText()
                } else {
                    Text(formatClockDuration(duration))
                }
            }
            .font(TextRole.duration(.title3).weight(.medium))
            .foregroundStyle(.primary)
            .fixedSize()
            WorklogActionsMenu(worklog: worklog)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("worklog-history.row.\(worklog.id)")
        .padding(Spacing.large)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: CornerRadius.medium))
        .overlay {
            RoundedRectangle(cornerRadius: CornerRadius.medium)
                .stroke(.quaternary, lineWidth: 1)
        }
        .contextMenu {
            WorklogContextMenu(worklog: worklog)
        }
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
            Label("Worklog actions", systemImage: "ellipsis.circle")
                .labelStyle(.iconOnly)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
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
