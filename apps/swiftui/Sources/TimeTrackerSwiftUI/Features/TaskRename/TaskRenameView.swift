import SwiftUI

struct TaskRenameSheet: View {
    let rename: TaskRenameStore

    var body: some View {
        let state = rename.state
        TaskNameSheet(
            title: "Edit task name", context: "Task: \(state.originalName)",
            name: Binding(
                get: { rename.state.name }, set: { rename.setName($0) }
            ), actionTitle: "Save",
            progressTitle: "Saving task...", isSubmitting: state.isSubmitting,
            canSubmit: state.canSubmit, error: state.error,
            cancel: rename.cancel, submit: rename.submit)
    }
}
