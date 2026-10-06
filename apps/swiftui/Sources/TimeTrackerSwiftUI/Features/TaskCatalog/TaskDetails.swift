import SwiftUI
import TrackerClient

struct TaskDetails: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let task = store.selectedTask {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .top, spacing: 16) {
                        Text(task.name)
                            .font(.system(size: 26, weight: .semibold))
                            .foregroundStyle(.primary)
                            .lineLimit(3)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                            .help(task.name)
                    }

                    HStack(spacing: 12) {
                        if task.archived {
                            Label("Archived", systemImage: "archivebox")
                                .foregroundStyle(.secondary)
                        } else if let active = store.active, active.taskId == task.id {
                            Label("Running since \(localTimestamp(active.start))", systemImage: "timer")
                                .foregroundStyle(.secondary)
                        } else {
                            Label("Active", systemImage: "checklist")
                                .foregroundStyle(.secondary)
                        }

                        if let status = store.autoPauseStatusText {
                            Text(status)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(24)

                Divider()
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        WorklogHistoryHeading(correction: store.correction)
                        errorView

                        if store.worklogs.isEmpty && store.error == nil {
                            Label("No worklogs yet", systemImage: "clock")
                                .foregroundStyle(.secondary)
                                .padding(.vertical, 12)
                        }

                        ForEach(store.worklogs) { worklog in
                            WorklogRow(worklog: worklog, active: store.active, timer: store.timer,
                                       correction: store.correction)
                        }

                        if store.hasMoreHistory {
                            HistoryPaginationButton(store: store, activity: store.activity)
                                .padding(.top, 8)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(24)
                }
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    errorView
                    Label("Select a task", systemImage: "checklist")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                    PendingWorklogCorrectionButton(correction: store.correction)
                }
                .padding(24)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.background)
    }

    @ViewBuilder
    private var errorView: some View {
        if let error = store.error {
            VStack(alignment: .leading, spacing: 8) {
                Label {
                    Text(error).textSelection(.enabled)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                }
                .foregroundStyle(.primary)
                if store.historyUnavailable {
                    Button("Retry") { store.retryHistory() }
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

private struct HistoryPaginationButton: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var activity: TrackerActivityStore

    var body: some View {
        Button("Load older worklogs") { store.loadOlder() }
            .disabled(activity.isBlockingControls || store.isStale)
    }
}
