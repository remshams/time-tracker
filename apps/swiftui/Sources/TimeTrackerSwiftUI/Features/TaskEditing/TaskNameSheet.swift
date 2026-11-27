import SwiftUI

/// The dialog that names a task, for creating and renaming one.
struct TaskNameSheet: View {
    let title: String
    let message: String
    let systemImage: String
    var tint = DialogTint.accent
    var footer: String? = nil
    @Binding var name: String
    let actionTitle: String
    let progressTitle: String
    let isSubmitting: Bool
    let canSubmit: Bool
    let error: String?
    let cancel: () -> Void
    let submit: () -> Void
    @FocusState private var nameFocused: Bool

    var body: some View {
        DialogSheet(
            title: title, message: message, systemImage: systemImage, tint: tint, error: error,
            errorIdentifier: "task-name.error", isBusy: isSubmitting
        ) {
            DialogSection(field: $nameFocused) {
                TextField("Task name", text: $name, prompt: Text("Task name"))
                    .textFieldStyle(.plain)
                    .font(TextRole.body)
                    .disableAutocorrection(true)
                    .focused($nameFocused)
                    .accessibilityIdentifier("task-name.input")
                    .disabled(isSubmitting)
            } footer: {
                if let footer { Text(footer) }
            }
        } status: {
            DelayedProgressIndicator(
                isBusy: isSubmitting, title: progressTitle,
                identifier: "task-name.progress")
        } actions: {
            Button("Cancel", action: cancel)
                .accessibilityIdentifier("task-name.cancel")
                .keyboardShortcut(.cancelAction)
                .disabled(isSubmitting)
            Button(actionTitle, action: submit)
                .accessibilityIdentifier("task-name.submit")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!canSubmit)
        }
        .onAppear { nameFocused = true }
    }
}
