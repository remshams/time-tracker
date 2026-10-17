import SwiftUI
import TrackerClient

struct WorklogRow: View {
    let session: TrackerSession
    let worklog: WorklogItem
    let active: WorklogItem?
    let timer: TrackerTimerStore
    let correction: WorklogCorrectionModel
    let move: WorklogMoveModel
    let deletion: WorklogDeletionModel

    private var duration: TimeInterval {
        guard let start = timestamp(worklog.start), let end = timestamp(worklog.end) else { return 0 }
        return max(0, end.timeIntervalSince(start))
    }

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
                    TrackerElapsedText(timer: timer)
                } else {
                    Text(clockDuration(duration))
                }
            }
            .font(.system(.title3, design: .monospaced).weight(.medium))
            .foregroundStyle(.primary)
            .monospacedDigit()
            .fixedSize()
            WorklogActionsMenu(
                session: session, correction: correction, move: move, deletion: deletion, worklog: worklog)
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
            WorklogContextMenu(
                session: session, correction: correction, move: move, deletion: deletion, worklog: worklog)
        }
    }
}

@MainActor
private struct WorklogActionsMenu: View {
    let session: TrackerSession
    @ObservedObject var correction: WorklogCorrectionModel
    @ObservedObject var move: WorklogMoveModel
    @ObservedObject var deletion: WorklogDeletionModel
    let worklog: WorklogItem

    var body: some View {
        Menu {
            WorklogContextMenu(
                session: session, correction: correction, move: move, deletion: deletion, worklog: worklog)
        } label: {
            Label("Worklog actions", systemImage: "ellipsis.circle")
                .labelStyle(.iconOnly)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .disabled(
            correction.state.isPresented || move.state.isPresented || deletion.state.isPresented
                || (!correction.canOpen && !move.canOpen && !deletion.canOpen)
        )
        .help("Worklog actions")
        .accessibilityIdentifier("worklog-history.actions.\(worklog.id)")
    }
}

@MainActor
private struct WorklogContextMenu: View {
    let session: TrackerSession
    @ObservedObject var correction: WorklogCorrectionModel
    @ObservedObject var move: WorklogMoveModel
    @ObservedObject var deletion: WorklogDeletionModel
    let worklog: WorklogItem

    var body: some View {
        Button("Edit times...") { session.openWorklogCorrection(worklogID: worklog.id) }
            .disabled(!correction.canOpen || correction.state.isPresented)
        Button("Move to task...") { session.openWorklogMove(worklogID: worklog.id) }
            .disabled(!move.canOpen || move.state.isPresented)
        Divider()
        // Running worklogs cannot be deleted. Stop the timer first.
        Button("Delete...", role: .destructive) { session.openWorklogDeletion(worklogID: worklog.id) }
            .disabled(!deletion.canOpen || deletion.state.isPresented || worklog.end == nil)
    }
}
