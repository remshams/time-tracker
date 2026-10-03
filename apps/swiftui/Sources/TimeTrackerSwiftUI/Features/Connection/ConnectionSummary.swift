import AppKit
import SwiftUI
import TrackerClient

struct ConnectionSummary: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: store.connectionSettings.mode == .local ? "internaldrive" : "network")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                Text(store.connectionSettings.mode == .local
                     ? "Local database" : store.connectionSettings.serverURL)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                    .help(store.connectionSettings.serverURL)
                Text(store.connectionStatusText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .help(store.connectionMessage ?? store.connectionStatusText)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if store.isBusy {
                ProgressView().controlSize(.small)
            } else if store.isStale {
                Button("Retry") { store.refresh() }
            }
            ConnectionSettingsButton(store: store)
                .buttonStyle(.borderless)
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 12)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}
