import SwiftUI

@MainActor
struct ConnectionActionsSection: View {
    @ObservedObject var draft: ConnectionDraftStore
    @ObservedObject var activity: TrackerActivityStore

    private var controlsDisabled: Bool { draft.operationPending || activity.isBlockingControls }

    var body: some View {
        Section {
            HStack(spacing: 12) {
                Button("Test connection") { draft.testConnection() }
                    .disabled(controlsDisabled || draft.missingEndpoint)
                    .accessibilityIdentifier("connection.test")
                Button("Connect") { draft.connect() }
                    .buttonStyle(.borderedProminent)
                    .disabled(controlsDisabled || draft.missingEndpoint)
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
