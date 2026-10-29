import SwiftUI
import TrackerClient

struct TaskArchivingSheet: View {
    let archiving: TaskArchivingStore

    var body: some View {
        let state = archiving.sheetContent
        let isUnarchiving = state.action == .unarchive
        let actionTitle = isUnarchiving ? "Unarchive" : "Archive"
        VStack(alignment: .leading, spacing: 16) {
            Text("\(actionTitle) task").font(.title2.bold())
            Text(state.taskName)
                .font(.headline)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Text(
                isUnarchiving
                    ? "The task will return to Active. Its worklog history will remain available."
                    : "The task will move to Archived. Its worklog history will remain available."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("task-archive.error")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: 12) {
                DelayedProgressIndicator(
                    isBusy: state.isSubmitting, title: isUnarchiving ? "Unarchiving task..." : "Archiving task...",
                    identifier: "task-archive.progress")
                Spacer()
                Button("Cancel") { archiving.cancel() }
                    .accessibilityIdentifier("task-archive.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button(actionTitle) {
                    archiving.submit()
                }
                .accessibilityIdentifier("task-archive.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!state.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 480)
        .interactiveDismissDisabled(state.isSubmitting)
    }
}
