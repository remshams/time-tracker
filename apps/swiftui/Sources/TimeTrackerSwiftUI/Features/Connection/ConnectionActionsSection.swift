import SwiftUI

@MainActor
struct ConnectionActionsSection: View {
    let draft: ConnectionDraftStore

    var body: some View {
        Section {
            HStack(spacing: 12) {
                Button("Test connection") { draft.testConnection() }
                    .disabled(draft.operationPending || draft.missingEndpoint)
                    .accessibilityIdentifier("connection.test")
                Button("Connect") { draft.connect() }
                    .buttonStyle(.borderedProminent)
                    .disabled(draft.operationPending || draft.missingEndpoint)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("connection.connect")
                DelayedProgressIndicator(
                    isBusy: draft.operationPending, identifier: "connection.progress")
            }

            if let resultMessage = draft.resultMessage {
                Label {
                    Text(resultMessage)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                } icon: {
                    Image(systemName: draft.resultSucceeded ? "checkmark.circle" : "exclamationmark.triangle")
                        .foregroundStyle(draft.resultSucceeded ? Color.accentColor : Color.red)
                }
                .accessibilityIdentifier("connection.result")
            }
        }
    }
}
