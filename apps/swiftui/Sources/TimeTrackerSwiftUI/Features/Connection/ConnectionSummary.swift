import SwiftUI
import TrackerClient

struct ConnectionSummary: View {
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        HStack(spacing: Spacing.medium) {
            Image(systemName: connection.settings.mode == .local ? "internaldrive" : "network")
                .foregroundStyle(
                    connection.stale || connection.changing
                        ? Color.secondary : ColorRole.connected)
            VStack(alignment: .leading, spacing: Spacing.xSmall) {
                Text(
                    connection.settings.mode == .local
                        ? "Local database" : connection.settings.serverURL
                )
                .font(TextRole.captionEmphasis)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(connection.settings.serverURL)
                .accessibilityIdentifier("connection-summary.source")
                Text(connection.statusText)
                    .font(TextRole.caption)
                    .foregroundStyle(.secondary)
                    .help(connection.message ?? connection.statusText)
                    .accessibilityIdentifier("connection-summary.status")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            DelayedProgressIndicator(
                isBusy: connection.changing, identifier: "connection-summary.progress")
            if !connection.changing, connection.stale {
                Button("Retry") { connection.refresh() }
                    .accessibilityIdentifier("connection-summary.retry")
            }
        }
        .padding(Spacing.medium)
    }
}
