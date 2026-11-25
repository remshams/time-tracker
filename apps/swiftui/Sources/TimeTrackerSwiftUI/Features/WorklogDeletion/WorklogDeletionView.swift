import SwiftUI
import TrackerClient

struct WorklogDeletionSheet: View {
    @Environment(WorklogDeletionStore.self) private var deletion

    var body: some View {
        DialogSheet(
            title: "Delete worklog", message: deletion.taskName, systemImage: "trash", tint: .destructive,
            width: DialogSize.wideWidth, error: deletion.error, errorIdentifier: "worklog-deletion.error",
            isBusy: deletion.isSubmitting
        ) {
            DialogSection {
                if let original = deletion.original, let start = parseTimestamp(original.start) {
                    WorklogSpanPanel(start: start, end: parseTimestamp(original.end), taskID: original.taskId)
                        .accessibilityElement(children: .combine)
                        .accessibilityIdentifier("worklog-deletion.summary")
                }
            } footer: {
                Text("The time is removed from this task and its totals. This can't be undone.")
            }
        } status: {
            DelayedProgressIndicator(
                isBusy: deletion.isSubmitting, title: "Deleting worklog...",
                identifier: "worklog-deletion.progress")
        } actions: {
            Button("Cancel") { deletion.cancel() }
                .accessibilityIdentifier("worklog-deletion.cancel")
                .keyboardShortcut(.cancelAction)
                .disabled(deletion.isSubmitting)
            Button("Delete", role: .destructive) { deletion.submit() }
                .accessibilityIdentifier("worklog-deletion.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .tint(ColorRole.error)
                .disabled(!deletion.canSubmit)
        }
    }
}
