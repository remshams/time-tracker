import SwiftUI
import TrackerClient

struct WorklogRow: View {
    let worklog: WorklogItem
    let active: WorklogItem?
    let correction: WorklogCorrectionStore

    private var duration: TimeInterval { worklogDuration(worklog) }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(localDay(worklog.start))
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.primary)
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 12)
            Group {
                if active?.id == worklog.id {
                    TrackerElapsedText()
                } else {
                    Text(formatClockDuration(duration))
                }
            }
            .font(.system(.title3, design: .monospaced).weight(.medium))
            .foregroundStyle(.primary)
            .monospacedDigit()
            .fixedSize()
            WorklogActionsMenu(correction: correction, worklog: worklog)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("worklog-history.row.\(worklog.id)")
        .padding(16)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(.quaternary, lineWidth: 1)
        }
        .contextMenu {
            WorklogContextMenu(correction: correction, worklog: worklog)
        }
    }
}

private struct WorklogActionsMenu: View {
    let correction: WorklogCorrectionStore
    let worklog: WorklogItem
    @Environment(WorklogMoveStore.self) private var move
    @Environment(TrackerTaskPresentationCoordinator.self) private var presentation
    @Environment(WorklogDeletionStore.self) private var deletion

    var body: some View {
        Menu {
            WorklogContextMenu(correction: correction, worklog: worklog)
        } label: {
            Label("Worklog actions", systemImage: "ellipsis.circle")
                .labelStyle(.iconOnly)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .disabled(presentation.hasPresentation || (!correction.canOpen && !move.canOpen && !deletion.canOpen))
        .help("Worklog actions")
        .accessibilityIdentifier("worklog-history.actions.\(worklog.id)")
    }
}

private struct WorklogContextMenu: View {
    let correction: WorklogCorrectionStore
    let worklog: WorklogItem
    @Environment(WorklogMoveStore.self) private var move
    @Environment(TrackerTaskPresentationCoordinator.self) private var presentation
    @Environment(WorklogDeletionStore.self) private var deletion

    var body: some View {
        let hasPresentation = presentation.hasPresentation
        Button("Edit times...") { correction.open(worklogID: worklog.id) }
            .disabled(!correction.canOpen || hasPresentation)
        Button("Move to task...") { move.open(worklogID: worklog.id) }
            .disabled(!move.canOpen || hasPresentation)
        Divider()
        // Running worklogs cannot be deleted. Stop the timer first.
        Button("Delete...", role: .destructive) { deletion.open(worklogID: worklog.id) }
            .disabled(!deletion.canOpen || hasPresentation || worklog.end == nil)
    }
}
