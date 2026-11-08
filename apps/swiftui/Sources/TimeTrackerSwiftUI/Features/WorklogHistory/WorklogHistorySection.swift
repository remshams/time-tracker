import SwiftUI
import TrackerClient

/// Shows the selected task's worklogs with their loading, empty, error and pagination states.
struct WorklogHistorySection: View {
    let store: TrackerStore
    @Environment(TrackingStore.self) private var tracking
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                WorklogHistoryHeading()
                WorklogHistoryError(store: store)

                if history.worklogs.isEmpty && history.isLoading {
                    DelayedProgressIndicator(
                        isBusy: history.isLoading, title: "Loading worklogs...",
                        identifier: "worklog-history.loading"
                    )
                    .padding(.vertical, 12)
                } else if history.worklogs.isEmpty && history.error == nil {
                    Label("No worklogs yet", systemImage: "clock")
                        .accessibilityIdentifier("worklog-history.empty")
                        .foregroundStyle(.secondary)
                        .padding(.vertical, 12)
                }

                ForEach(history.worklogs) { worklog in
                    WorklogRow(worklog: worklog, active: tracking.active)
                }

                if history.hasMore {
                    HistoryPaginationButton(store: store)
                        .padding(.top, 8)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(24)
        }
        .accessibilityIdentifier("worklog-history.scroll")
    }
}

/// Shows the history panel's message, with a retry button when the history failed to load.
struct WorklogHistoryError: View {
    let store: TrackerStore
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        if let error = history.error {
            VStack(alignment: .leading, spacing: 8) {
                Label {
                    Text(error).textSelection(.enabled)
                        .accessibilityIdentifier("worklog-history.error")
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                }
                .foregroundStyle(.primary)
                if history.unavailable {
                    Button("Retry") { store.retryHistory() }
                        .accessibilityIdentifier("worklog-history.retry")
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            .overlay {
                RoundedRectangle(cornerRadius: 8)
                    .stroke(.quaternary, lineWidth: 1)
            }
        }
    }
}

private struct WorklogHistoryHeading: View {
    var body: some View {
        Text("Worklogs").font(.title3.weight(.semibold))
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.bottom, 4)
    }
}

private struct HistoryPaginationButton: View {
    let store: TrackerStore
    @Environment(WorklogHistoryStore.self) private var history
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        Button("Load older worklogs") { store.loadOlder() }
            .accessibilityIdentifier("worklog-history.load-older")
            .disabled(connection.stale || history.isLoading)
    }
}
