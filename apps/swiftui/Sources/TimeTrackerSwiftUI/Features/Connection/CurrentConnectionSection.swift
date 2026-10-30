import SwiftUI
import TrackerClient

struct CurrentConnectionSection: View {
    let store: TrackerStore

    var body: some View {
        Section("Current connection") {
            LabeledContent("Data source", value: store.connectionSettings.mode.label)
            if store.connectionSettings.mode == .server {
                Text(store.connectionSettings.serverURL)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(store.connectionStatusText)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("connection.status")
            if let message = store.connectionMessage {
                Text(message)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}
