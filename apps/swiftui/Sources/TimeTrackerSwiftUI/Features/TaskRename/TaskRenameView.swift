import SwiftUI
import TrackerClient

struct TaskRenameSheet: View {
    @Environment(TaskRenameStore.self) private var rename

    var body: some View {
        TaskNameSheet(
            title: "Edit task name", message: rename.originalName, systemImage: "pencil",
            tint: .task(id: rename.taskID), footer: "The task keeps its worklogs and totals.",
            name: Binding(
                get: { rename.name }, set: { rename.setName($0) }
            ), actionTitle: "Save",
            progressTitle: "Saving task...", isSubmitting: rename.isSubmitting,
            canSubmit: rename.canSubmit, error: rename.error,
            cancel: rename.cancel, submit: rename.submit)
    }
}
