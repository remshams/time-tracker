import SwiftUI
import TrackerClient

struct WorklogRow: View {
    let worklog: WorklogItem
    let active: WorklogItem?
    let timer: TrackerTimerStore
    let correction: WorklogCorrectionStore

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
            WorklogEditButton(correction: correction, worklogID: worklog.id)
        }
        .padding(16)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(.quaternary, lineWidth: 1)
        }
        .contextMenu {
            WorklogContextMenu(correction: correction, worklogID: worklog.id)
        }
    }
}

@MainActor
private struct WorklogEditButton: View {
    @ObservedObject var correction: WorklogCorrectionStore
    let worklogID: String

    var body: some View {
        Button {
            correction.open(worklogID: worklogID)
        } label: {
            Label("Edit times...", systemImage: "pencil")
                .labelStyle(.iconOnly)
        }
        .buttonStyle(.borderless)
        .disabled(!correction.canOpen || correction.state.isPresented)
        .help("Edit worklog times")
    }
}

@MainActor
private struct WorklogContextMenu: View {
    @ObservedObject var correction: WorklogCorrectionStore
    let worklogID: String

    var body: some View {
        Button("Edit times...") { correction.open(worklogID: worklogID) }
            .disabled(!correction.canOpen || correction.state.isPresented)
    }
}
