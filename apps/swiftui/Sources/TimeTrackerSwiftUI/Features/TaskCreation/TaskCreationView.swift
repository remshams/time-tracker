import SwiftUI
import TrackerClient

struct TaskCreationSheet: View {
    let creation: TaskCreationStore

    var body: some View {
        let state = creation.state
        TaskNameSheet(
            title: "New task",
            name: Binding(
                get: { creation.state.name }, set: { creation.setName($0) }
            ), actionTitle: "Create",
            progressTitle: "Creating task...", isSubmitting: state.isSubmitting,
            canSubmit: state.canSubmit, error: state.error,
            cancel: creation.cancel, submit: creation.submit)
    }
}
