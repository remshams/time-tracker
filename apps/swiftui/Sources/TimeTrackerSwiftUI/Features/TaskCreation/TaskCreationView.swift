import SwiftUI
import TrackerClient

@MainActor
struct TaskCreationDialog: View {
    @ObservedObject var creation: TaskCreationStore

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(isPresented: Binding(
                get: { creation.state.isPresented },
                set: { if !$0 { creation.cancel() } }
            )) {
                TaskCreationSheet(creation: creation)
            }
    }
}

@MainActor
private struct TaskCreationSheet: View {
    @ObservedObject var creation: TaskCreationStore

    var body: some View {
        let state = creation.state
        TaskNameSheet(title: "New task", name: Binding(
            get: { creation.state.name }, set: { creation.setName($0) }
        ), actionTitle: state.error != nil && !state.canEditName ? "Retry" : "Create",
                      progressTitle: "Creating task...", isSubmitting: state.isSubmitting,
                      canEditName: state.canEditName, canSubmit: state.canSubmit, error: state.error,
                      cancel: creation.cancel, submit: creation.submit)
    }
}

@MainActor
struct TaskCreationCommands: Commands {
    @FocusedObject private var creation: TaskCreationStore?

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New task") { creation?.open() }
                .keyboardShortcut("n", modifiers: .command)
                .disabled(creation?.canOpen != true)
        }
    }
}
