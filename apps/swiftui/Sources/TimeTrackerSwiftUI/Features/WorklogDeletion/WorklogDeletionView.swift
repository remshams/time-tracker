import SwiftUI
import TrackerClient

struct WorklogDeletionSheet: View {
    @Environment(WorklogDeletionStore.self) private var deletion

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.large) {
            Text("Delete worklog").font(TextRole.sheetTitle)
            Text(deletion.taskName)
                .font(TextRole.detail)
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
                    .foregroundStyle(ColorRole.error)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: Spacing.medium) {
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
                .tint(ColorRole.error)
                .disabled(!deletion.canSubmit)
            }
        }
        .padding(Spacing.xLarge)
        .frame(width: 440)
        .interactiveDismissDisabled(deletion.isSubmitting)
    }
}

/// Shows the entry that will be removed, matching the history row's layout.
private struct WorklogDeletionSummary: View {
    let worklog: WorklogItem

    private var duration: TimeInterval { worklogDuration(worklog) }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.medium) {
            VStack(alignment: .leading, spacing: Spacing.xSmall) {
                Text(localDay(worklog.start))
                    .font(TextRole.detailEmphasis)
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: Spacing.medium)
            DurationLabel(duration: DurationText(duration: duration))
                .font(TextRole.duration(.title3).weight(.medium))
                .accessibilityLabel("Duration \(spokenDuration(Int(max(0, duration))))")
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("worklog-deletion.summary")
        .padding(Spacing.large)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: CornerRadius.medium))
        .overlay {
            RoundedRectangle(cornerRadius: CornerRadius.medium)
                .stroke(.quaternary, lineWidth: BorderWidth.hairline)
        }
    }
}
