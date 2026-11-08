import SwiftUI
import TrackerClient

struct TaskCreationSheet: View {
    @Environment(TaskCreationStore.self) private var creation

    var body: some View {
        TaskNameSheet(
            title: "New task",
            name: Binding(
                get: { creation.name }, set: { creation.setName($0) }
            ), actionTitle: "Create",
            progressTitle: "Creating task...", isSubmitting: creation.isSubmitting,
            canSubmit: creation.canSubmit, error: creation.error,
            cancel: creation.cancel, submit: creation.submit)
    }
}
