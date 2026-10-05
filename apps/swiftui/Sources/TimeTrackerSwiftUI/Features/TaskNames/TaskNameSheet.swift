import SwiftUI

@MainActor
struct TaskNameSheet: View {
    let title: String
    var context: String? = nil
    @Binding var name: String
    let actionTitle: String
    let progressTitle: String
    let isSubmitting: Bool
    let canEditName: Bool
    let canSubmit: Bool
    let error: String?
    let cancel: () -> Void
    let submit: () -> Void
    @FocusState private var nameFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(title).font(.title2.bold())
            if let context {
                Text(context)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .help(context)
            }
            TextField("Task name", text: $name)
                .textFieldStyle(.roundedBorder)
                .disableAutocorrection(true)
                .focused($nameFocused)
                .disabled(!canEditName)

            if let error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(Color(nsColor: .systemRed))
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: 12) {
                if isSubmitting {
                    ProgressView().controlSize(.small)
                    Text(progressTitle).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel", action: cancel)
                    .keyboardShortcut(.cancelAction)
                    .disabled(isSubmitting)
                Button(actionTitle, action: submit)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(!canSubmit)
            }
        }
        .padding(24)
        .frame(width: 440)
        .interactiveDismissDisabled(isSubmitting)
        .onAppear { nameFocused = true }
    }
}
