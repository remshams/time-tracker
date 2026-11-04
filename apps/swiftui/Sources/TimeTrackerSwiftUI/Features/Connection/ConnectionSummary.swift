import SwiftUI
import TrackerClient

struct ConnectionSummary: View {
    let store: TrackerStore
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: connection.settings.mode == .local ? "internaldrive" : "network")
                .foregroundStyle(
                    connection.stale || connection.changing
                        ? Color.secondary : Color.green)
            VStack(alignment: .leading, spacing: 3) {
                Text(
                    connection.settings.mode == .local
                        ? "Local database" : connection.settings.serverURL
                )
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(connection.settings.serverURL)
                .accessibilityIdentifier("connection-summary.source")
                Text(connection.statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .help(connection.message ?? connection.statusText)
                    .accessibilityIdentifier("connection-summary.status")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            DelayedProgressIndicator(
                isBusy: connection.changing, identifier: "connection-summary.progress")
            if !connection.changing, connection.stale {
                Button("Retry") { store.refresh() }
                    .accessibilityIdentifier("connection-summary.retry")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }
}
