import SwiftUI

@MainActor
struct TaskRenameDialog: View {
    @ObservedObject var rename: TaskRenameStore
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(isPresented: Binding(
                get: { isPresentationOwner && rename.state.isPresented },
                set: { if isPresentationOwner && !$0 { rename.cancel() } }
            )) {
                TaskRenameSheet(rename: rename)
            }
    }
}

@MainActor
private struct TaskRenameSheet: View {
    @ObservedObject var rename: TaskRenameStore

    var body: some View {
        let state = rename.state
        TaskNameSheet(title: "Edit task name", context: "Task: \(state.originalName)", name: Binding(
            get: { rename.state.name }, set: { rename.setName($0) }
        ), actionTitle: state.error != nil && !state.canEditName && state.canSubmit ? "Retry" : "Save",
                      progressTitle: "Saving task...", isSubmitting: state.isSubmitting,
                      canEditName: state.canEditName, canSubmit: state.canSubmit, error: state.error,
                      cancel: rename.cancel, submit: rename.submit)
    }
}
