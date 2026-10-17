import SwiftUI
import TrackerClient

@MainActor
struct TaskRenameDialog: View {
    let session: TrackerSession
    @ObservedObject var rename: TaskRenameModel
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(
                isPresented: Binding(
                    get: { isPresentationOwner && rename.state.isPresented },
                    set: { if isPresentationOwner && !$0 { session.cancelTaskRename() } }
                )
            ) {
                TaskRenameSheet(session: session, rename: rename)
            }
    }
}

@MainActor
private struct TaskRenameSheet: View {
    let session: TrackerSession
    @ObservedObject var rename: TaskRenameModel

    var body: some View {
        let state = rename.sheetContent
        TaskNameSheet(
            title: "Edit task name", context: "Task: \(state.originalName)",
            name: Binding(
                get: { rename.sheetContent.name }, set: { session.setTaskRenameName($0) }
            ), actionTitle: state.error != nil && !state.canEditName && state.canSubmit ? "Retry" : "Save",
            progressTitle: "Saving task...", isSubmitting: state.isSubmitting,
            canEditName: state.canEditName, canSubmit: state.canSubmit, error: state.error,
            cancel: session.cancelTaskRename, submit: session.submitTaskRename)
    }
}
