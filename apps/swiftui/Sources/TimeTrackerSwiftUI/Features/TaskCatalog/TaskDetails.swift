import AppKit
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
                        Spacer(minLength: 0)
                        TaskRenameButton(rename: store.rename, taskID: task.id)
                            .fixedSize()
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

                        Spacer(minLength: 12)
                        if !task.archived {
                            TrackingTaskButton(store: store, activity: store.activity, taskID: task.id)
                                .fixedSize()
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(24)

                Divider()
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        Text("Worklogs")
                            .font(.title3.weight(.semibold))
                            .padding(.bottom, 4)
                        errorView

                        if store.worklogs.isEmpty && store.error == nil {
                            Label("No worklogs yet", systemImage: "clock")
                                .foregroundStyle(.secondary)
                                .padding(.vertical, 12)
                        }

                        ForEach(store.worklogs) { worklog in
                            WorklogRow(worklog: worklog, active: store.active, timer: store.timer)
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
                }
                .padding(24)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(nsColor: .windowBackgroundColor))
        .navigationTitle(store.selectedTask?.name ?? "Time Tracker")
    }

    @ViewBuilder
    private var errorView: some View {
        if let error = store.error {
            VStack(alignment: .leading, spacing: 8) {
                Label {
                    Text(error).textSelection(.enabled)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(Color(nsColor: .systemRed))
                }
                .foregroundStyle(.primary)
                if store.historyUnavailable {
                    Button("Retry") { store.retryHistory() }
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay {
                RoundedRectangle(cornerRadius: 8)
                    .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
            }
        }
    }
}

private struct TrackingTaskButton: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var activity: TrackerActivityStore
    let taskID: String

    var body: some View {
        if let active = store.active, active.taskId == taskID {
            Button { store.stopTracking(worklogID: active.id) } label: {
                Label("Stop tracking", systemImage: "stop.fill")
            }
            .buttonStyle(.bordered)
            .disabled(!activity.canStopTracking)
            .help("Stop the running timer and save this worklog.")
        } else {
            Button { store.startTracking(taskID: taskID) } label: {
                Label(store.active == nil ? "Start tracking" : "Switch tracking", systemImage: "play.fill")
            }
            .buttonStyle(.borderedProminent)
            .disabled(!activity.canStartSelectedTask)
            .help(store.active == nil
                  ? "Start a timer for this task."
                  : "Stop the current timer and start tracking this task.")
        }
    }
}

private struct HistoryPaginationButton: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var activity: TrackerActivityStore

    var body: some View {
        Button("Load older worklogs") { store.loadOlder() }
            .disabled(activity.isBusy || store.isStale)
    }
}
