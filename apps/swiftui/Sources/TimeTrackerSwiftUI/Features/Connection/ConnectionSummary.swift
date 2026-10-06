import SwiftUI
import TrackerClient

@MainActor
struct ConnectionSummary: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject private var activity: TrackerActivityStore

    init(store: TrackerStore) {
        self.store = store
        activity = store.activity
    }

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: store.connectionSettings.mode == .local ? "internaldrive" : "network")
                .foregroundStyle(store.isStale || store.isChangingConnection
                                 ? Color.secondary : Color.green)
            VStack(alignment: .leading, spacing: 3) {
                Text(store.connectionSettings.mode == .local
                     ? "Local database" : store.connectionSettings.serverURL)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(store.connectionSettings.serverURL)
                Text(store.connectionStatusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .help(store.connectionMessage ?? store.connectionStatusText)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if store.isChangingConnection {
                ProgressView().controlSize(.small)
            } else if store.isStale {
                Button("Retry") { store.refresh() }
                    .disabled(activity.isBlockingControls)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
    }
}
