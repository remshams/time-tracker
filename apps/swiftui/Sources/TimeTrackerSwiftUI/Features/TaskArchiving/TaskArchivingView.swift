import SwiftUI
import TrackerClient

struct TaskArchivingSheet: View {
    @Environment(TaskArchivingStore.self) private var archiving

    var body: some View {
        let isUnarchiving = archiving.action == .unarchive
        let actionTitle = isUnarchiving ? "Unarchive" : "Archive"
        VStack(alignment: .leading, spacing: Spacing.large) {
            Text("\(actionTitle) task").font(TextRole.sheetTitle)
            Text(archiving.taskName)
                .font(TextRole.heading)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Text(
                isUnarchiving
                    ? "The task will return to Active. Its worklog history will remain available."
                    : "The task will move to Archived. Its worklog history will remain available."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)

            if let error = archiving.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("task-archive.error")
                    .foregroundStyle(ColorRole.error)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: Spacing.medium) {
                DelayedProgressIndicator(
                    isBusy: archiving.isSubmitting, title: isUnarchiving ? "Unarchiving task..." : "Archiving task...",
                    identifier: "task-archive.progress")
                Spacer()
                Button("Cancel") { archiving.cancel() }
                    .accessibilityIdentifier("task-archive.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(archiving.isSubmitting)
                Button(actionTitle) {
                    archiving.submit()
                }
                .accessibilityIdentifier("task-archive.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!archiving.canSubmit)
            }
        }
        .padding(Spacing.xLarge)
        .frame(width: 480)
        .interactiveDismissDisabled(archiving.isSubmitting)
    }
}
