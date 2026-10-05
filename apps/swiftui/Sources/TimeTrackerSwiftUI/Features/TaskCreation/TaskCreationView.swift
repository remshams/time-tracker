import SwiftUI
import TrackerClient

@MainActor
struct TaskCreationButton: View {
    @ObservedObject var creation: TaskCreationStore

    var body: some View {
        Button(action: creation.open) {
            Label("New task", systemImage: "plus")
        }
        .disabled(!creation.canOpen)
        .help("Create a task")
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
    @FocusState private var nameFocused: Bool

    var body: some View {
        let state = creation.state
        VStack(alignment: .leading, spacing: 16) {
            Text("New task")
                .font(.title2.bold())
            TextField("Task name", text: Binding(
                get: { creation.state.name },
                set: { creation.setName($0) }
            ), prompt: Text("What are you working on?"))
            .textFieldStyle(.roundedBorder)
            .disableAutocorrection(true)
            .focused($nameFocused)
            .disabled(!state.canEditName)

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(Color(nsColor: .systemRed))
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: 12) {
                if state.isSubmitting {
                    ProgressView().controlSize(.small)
                    Text("Creating task...")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel", action: creation.cancel)
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button(state.error != nil && !state.canEditName ? "Retry" : "Create", action: creation.submit)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(!state.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 440)
        .interactiveDismissDisabled(state.isSubmitting)
        .onAppear { nameFocused = true }
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
