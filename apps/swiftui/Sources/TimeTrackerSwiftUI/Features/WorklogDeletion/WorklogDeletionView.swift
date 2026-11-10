import SwiftUI
import TrackerClient

struct WorklogDeletionSheet: View {
    @Environment(WorklogDeletionStore.self) private var deletion

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Delete worklog").font(.title2.bold())
            Text(deletion.taskName)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(3)
                .help(deletion.taskName)

            if let original = deletion.original {
                WorklogDeletionSummary(worklog: original)
            }

            Text("The time is removed from this task and its totals. This can't be undone.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if let error = deletion.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("worklog-deletion.error")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: 12) {
                DelayedProgressIndicator(
                    isBusy: deletion.isSubmitting, title: "Deleting worklog...",
                    identifier: "worklog-deletion.progress")
                Spacer()
                Button("Cancel") { deletion.cancel() }
                    .accessibilityIdentifier("worklog-deletion.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(deletion.isSubmitting)
                Button("Delete", role: .destructive) {
                    deletion.submit()
                }
                .accessibilityIdentifier("worklog-deletion.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .tint(.red)
                .disabled(!deletion.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 440)
        .interactiveDismissDisabled(deletion.isSubmitting)
    }
}

/// Shows the entry that will be removed, matching the history row's layout.
private struct WorklogDeletionSummary: View {
    let worklog: WorklogItem

    private var duration: TimeInterval { worklogDuration(worklog) }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(localDay(worklog.start))
                    .font(.subheadline.weight(.medium))
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 12)
            Text(formatClockDuration(duration))
                .font(.system(.title3, design: .monospaced).weight(.medium))
                .monospacedDigit()
                .accessibilityLabel("Duration \(formatClockDuration(duration))")
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("worklog-deletion.summary")
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(.quaternary, lineWidth: 1)
        }
    }
}
