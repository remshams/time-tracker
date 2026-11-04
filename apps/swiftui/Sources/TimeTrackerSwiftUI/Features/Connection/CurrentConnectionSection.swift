import SwiftUI
import TrackerClient

struct CurrentConnectionSection: View {
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        Section("Current connection") {
            LabeledContent("Data source", value: connection.settings.mode.label)
            if connection.settings.mode == .server {
                Text(connection.settings.serverURL)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(connection.statusText)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("connection.status")
            if let message = connection.message {
                Text(message)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}
