import SwiftUI
import TrackerClient

/// Shows the selected task's worklogs with their loading, empty, error and pagination states.
struct WorklogHistorySection: View {
    @Environment(TrackingStore.self) private var tracking
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: Spacing.medium) {
                WorklogHistoryHeading()
                WorklogHistoryError()

                if history.worklogs.isEmpty && history.isLoading {
                    DelayedProgressIndicator(
                        isBusy: history.isLoading, title: "Loading worklogs...",
                        identifier: "worklog-history.loading"
                    )
                    .padding(.vertical, Spacing.medium)
                } else if history.worklogs.isEmpty && history.error == nil {
                    Label("No worklogs yet", systemImage: "clock")
                        .accessibilityIdentifier("worklog-history.empty")
                        .foregroundStyle(.secondary)
                        .padding(.vertical, Spacing.medium)
                }

                ForEach(history.worklogs) { worklog in
                    WorklogRow(worklog: worklog, active: tracking.active)
                }

                if history.hasMore {
                    HistoryPaginationButton()
                        .padding(.top, Spacing.small)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.xLarge)
        }
        .accessibilityIdentifier("worklog-history.scroll")
    }
}

/// Shows the history panel's message, with a retry button when the history failed to load.
struct WorklogHistoryError: View {
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        if let error = history.error {
            VStack(alignment: .leading, spacing: Spacing.small) {
                Label {
                    Text(error).textSelection(.enabled)
                        .accessibilityIdentifier("worklog-history.error")
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(ColorRole.error)
                }
                .foregroundStyle(.primary)
                if history.unavailable {
                    Button("Retry") { history.reload() }
                        .accessibilityIdentifier("worklog-history.retry")
                }
            }
            .padding(Spacing.medium)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: CornerRadius.medium))
            .overlay {
                RoundedRectangle(cornerRadius: CornerRadius.medium)
                    .stroke(.quaternary, lineWidth: BorderWidth.hairline)
            }
        }
    }
}

private struct WorklogHistoryHeading: View {
    var body: some View {
        Text("Worklogs").font(TextRole.sectionTitle)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.bottom, Spacing.xSmall)
    }
}

private struct HistoryPaginationButton: View {
    @Environment(WorklogHistoryStore.self) private var history
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        Button("Load older worklogs") { history.loadOlder() }
            .accessibilityIdentifier("worklog-history.load-older")
            .disabled(connection.stale || history.isLoading)
    }
}
