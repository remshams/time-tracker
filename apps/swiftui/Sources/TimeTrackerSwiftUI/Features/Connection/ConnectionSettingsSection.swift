import SwiftUI
import TrackerClient

@MainActor
struct ConnectionSettingsSection: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var draft: ConnectionDraftStore

    var body: some View {
        Section("Connection") {
            Picker("Data source", selection: $draft.settings.mode) {
                ForEach(ConnectionMode.allCases) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .accessibilityIdentifier("connection.mode")

            if draft.settings.mode == .server {
                TextField(
                    "Server URL", text: $draft.settings.serverURL,
                    prompt: Text("http://server-address:port")
                )
                .textFieldStyle(.roundedBorder)
                .disableAutocorrection(true)
                .accessibilityIdentifier("connection.server-url")
                Text("Enter the full HTTP or HTTPS address of your tracker server.")
                    .foregroundStyle(.secondary)
            } else {
                Text("Use the task database stored on this Mac.")
                    .foregroundStyle(.secondary)
            }
        }
        .disabled(draft.operationPending || store.isChangingConnection)
    }
}
