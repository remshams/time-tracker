import SwiftUI

struct TaskNameSheet: View {
    let title: String
    var context: String? = nil
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
        VStack(alignment: .leading, spacing: Spacing.large) {
            Text(title).font(TextRole.sheetTitle)
            if let context {
                Text(context)
                    .font(TextRole.detail)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .help(context)
            }
            TextField("Task name", text: $name)
                .textFieldStyle(.roundedBorder)
                .disableAutocorrection(true)
                .focused($nameFocused)
                .accessibilityIdentifier("task-name.input")
                .disabled(isSubmitting)

            if let error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("task-name.error")
                    .foregroundStyle(ColorRole.error)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: Spacing.medium) {
                DelayedProgressIndicator(
                    isBusy: isSubmitting, title: progressTitle,
                    identifier: "task-name.progress")
                Spacer()
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
        }
        .padding(Spacing.xLarge)
        .frame(width: 440)
        .interactiveDismissDisabled(isSubmitting)
        .onAppear { nameFocused = true }
    }
}
