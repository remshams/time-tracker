import AppKit
import SwiftUI
import TrackerClient

struct TrackerWindow: View {
    @ObservedObject var store: TrackerStore
    let statusItem: TrackerStatusItemController
    @Environment(\.openWindow) private var openWindow

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { store.tab }, set: { store.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { store.selectedTaskID }, set: { store.select($0) })
    }

    private var trackingFailurePresented: Binding<Bool> {
        Binding(get: { store.trackingError != nil }, set: {
            if !$0 { store.dismissTrackingError() }
        })
    }

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                Picker("Tasks", selection: tabBinding) {
                    ForEach(TaskTab.allCases) { tab in
                        Text(tab.rawValue).tag(tab)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(12)

                HStack {
                    Text("Tasks")
                    Spacer()
                    Text(store.dailyTotalsStatus == .cached ? "Today, cached" : "Today")
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 16)
                .padding(.bottom, 4)

                List(selection: selectionBinding) {
                    ForEach(store.visibleTasks) { task in
                        HStack(spacing: 8) {
                            TaskIndicatorView(taskID: task.id,
                                              isRunning: !store.isStale && store.active?.taskId == task.id)
                            Text(task.name)
                                .lineLimit(2)
                                .padding(.vertical, 4)
                            Spacer(minLength: 4)
                            TrackerDailyTotalText(totals: store.dailyTotals, taskID: task.id)
                        }
                        .tag(task.id)
                        .help(task.name)
                        .contextMenu {
                            TaskContextMenu(creation: store.creation, rename: store.rename, taskID: task.id)
                        }
                    }
                }
                .listStyle(.sidebar)
                .contextMenu {
                    TaskContextMenu(creation: store.creation, rename: store.rename, taskID: nil)
                }
                .overlay {
                    if store.visibleTasks.isEmpty {
                        VStack(spacing: 8) {
                            Image(systemName: "tray")
                            Text("No \(store.tab.rawValue.lowercased()) tasks")
                        }
                        .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Tasks")
            .navigationSplitViewColumnWidth(min: 280, ideal: 340, max: 480)
            .toolbar {
                ToolbarItemGroup(placement: .navigation) {
                    ConnectionSettingsButton(store: store)
                    TaskCreationButton(creation: store.creation)
                }
            }
        } detail: {
            VStack(spacing: 0) {
                ConnectionSummary(store: store)
                Divider()
                TaskDetails(store: store)
            }
            .toolbar {
                ToolbarItem(placement: .principal) {
                    TrackerToolbarTitle(store: store)
                }
                ToolbarItemGroup(placement: .primaryAction) {
                    if let task = store.selectedTask {
                        TaskRenameButton(rename: store.rename, taskID: task.id)
                        if !task.archived {
                            TrackingTaskButton(store: store, activity: store.activity, taskID: task.id)
                        }
                    }
                }
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        .focusedSceneObject(store.creation)
        .background {
            TaskCreationDialog(creation: store.creation)
            TaskRenameDialog(rename: store.rename)
        }
        .onAppear {
            statusItem.start { openWindow(id: "tracker") }
        }
        .alert("Could not change tracking", isPresented: trackingFailurePresented) {
            Button("OK", role: .cancel) { store.dismissTrackingError() }
        } message: {
            Text(store.trackingError ?? "Please try again.")
        }
    }
}

private struct TrackerToolbarTitle: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        HStack(spacing: 12) {
            Text(store.selectedTask?.name ?? "Time Tracker")
                .font(.headline)
                .lineLimit(1)
                .help(store.selectedTask?.name ?? "Time Tracker")
            if store.active != nil {
                TrackerElapsedText(timer: store.timer)
                    .font(.body.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .help(store.isStale ? "Last confirmed timer: \(store.runningTaskName)"
                          : "Tracking: \(store.runningTaskName)")
            }
        }
    }
}

struct TrackerDailyTotalText: View {
    @ObservedObject var totals: TrackerDailyTotalsStore
    let taskID: String

    var body: some View {
        Text(totals.text(taskID: taskID))
            .font(.callout.monospacedDigit())
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
            .help(totals.explanation)
            .accessibilityLabel("Today's total: \(totals.text(taskID: taskID))")
    }
}
