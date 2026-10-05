import AppKit
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
                                 ? Color.secondary : Color(nsColor: .systemGreen))
            VStack(alignment: .leading, spacing: 3) {
                Text(store.connectionSettings.mode == .local
                     ? "Local database" : store.connectionSettings.serverURL)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .help(store.connectionSettings.serverURL)
                Text(store.connectionStatusText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .help(store.connectionMessage ?? store.connectionStatusText)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if activity.isBusy {
                ProgressView().controlSize(.small)
            } else if store.isStale {
                Button("Retry") { store.refresh() }
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 12)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}
