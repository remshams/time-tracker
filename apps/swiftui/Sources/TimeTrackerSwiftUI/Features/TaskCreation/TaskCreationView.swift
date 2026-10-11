import SwiftUI
import TrackerClient

@MainActor
struct TaskCreationDialog: View {
    let session: TrackerSession
    @ObservedObject var creation: TaskCreationModel
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(
                isPresented: Binding(
                    get: { isPresentationOwner && creation.state.isPresented },
                    set: { if isPresentationOwner && !$0 { session.cancelTaskCreation() } }
                )
            ) {
                TaskCreationSheet(session: session, creation: creation)
            }
    }
}

@MainActor
private struct TaskCreationSheet: View {
    let session: TrackerSession
    @ObservedObject var creation: TaskCreationModel

    var body: some View {
        let state = creation.sheetContent
        TaskNameSheet(
            title: "New task",
            name: Binding(
                get: { creation.sheetContent.name }, set: { session.setTaskCreationName($0) }
            ), actionTitle: state.error != nil && !state.canEditName ? "Retry" : "Create",
            progressTitle: "Creating task...", isSubmitting: state.isSubmitting,
            canEditName: state.canEditName, canSubmit: state.canSubmit, error: state.error,
            cancel: session.cancelTaskCreation, submit: session.submitTaskCreation)
    }
}
