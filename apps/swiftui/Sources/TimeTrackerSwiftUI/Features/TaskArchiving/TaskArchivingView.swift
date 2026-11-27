import SwiftUI
import TrackerClient

struct TaskArchivingSheet: View {
    @Environment(TaskArchivingStore.self) private var archiving

    var body: some View {
        let isUnarchiving = archiving.action == .unarchive
        let actionTitle = isUnarchiving ? "Unarchive" : "Archive"
        DialogSheet(
            title: "\(actionTitle) task",
            message: isUnarchiving ? "Return it to the active tasks." : "Move it out of the active tasks.",
            systemImage: isUnarchiving ? "tray.and.arrow.up" : "archivebox", tint: .task(id: archiving.taskID),
            error: archiving.error, errorIdentifier: "task-archive.error", isBusy: archiving.isSubmitting
        ) {
            DialogSection {
                Text(archiving.taskName)
                    .font(TextRole.heading)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            } footer: {
                Text("Its worklog history remains available.")
            }
        } status: {
            DelayedProgressIndicator(
                isBusy: archiving.isSubmitting, title: isUnarchiving ? "Unarchiving task..." : "Archiving task...",
                identifier: "task-archive.progress")
        } actions: {
            Button("Cancel") { archiving.cancel() }
                .accessibilityIdentifier("task-archive.cancel")
                .keyboardShortcut(.cancelAction)
                .disabled(archiving.isSubmitting)
            Button(actionTitle) { archiving.submit() }
                .accessibilityIdentifier("task-archive.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!archiving.canSubmit)
        }
    }
}
