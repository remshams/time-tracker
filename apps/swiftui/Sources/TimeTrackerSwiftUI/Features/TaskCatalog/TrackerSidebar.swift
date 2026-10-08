import SwiftUI
import TrackerClient

@MainActor
struct TrackerSidebar: View {
    @ObservedObject var store: TrackerStore

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { store.tab }, set: { store.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { store.selectedTaskID }, set: { store.select($0) })
    }

    var body: some View {
        VStack(spacing: 0) {
            Picker("Tasks", selection: tabBinding) {
                ForEach(TaskTab.allCases) { tab in
                    Text(tab.rawValue).tag(tab)
                        .accessibilityIdentifier("task-sidebar.tab.\(tab.rawValue)")
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
                            .accessibilityIdentifier(!store.isStale && store.active?.taskId == task.id
                                                     ? "tracking.active-task" : "task-sidebar.indicator.\(task.id)")
                        Text(task.name)
                            .lineLimit(2)
                            .padding(.vertical, 4)
                        Spacer(minLength: 4)
                        TrackerDailyTotalText(totals: store.dailyTotals, taskID: task.id)
                    }
                    .tag(task.id)
                    .help(task.name)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("task-sidebar.task.\(task.id)")
                    .contextMenu {
                        TaskContextMenu(creation: store.creation, rename: store.rename,
                                        archiving: store.archiving, taskID: task.id,
                                        taskIsArchived: task.archived,
                                        taskIsRunning: store.active?.taskId == task.id)
                    }
                }
            }
            .listStyle(.sidebar)
            .accessibilityIdentifier("task-sidebar.list")
            .contextMenu {
                TaskContextMenu(creation: store.creation, rename: store.rename,
                                archiving: store.archiving, taskID: nil)
            }
            .overlay {
                if store.visibleTasks.isEmpty {
                    VStack(spacing: 8) {
                        Image(systemName: "tray")
                        Text("No \(store.tab.rawValue.lowercased()) tasks")
                            .accessibilityIdentifier("task-sidebar.empty")
                    }
                    .foregroundStyle(.secondary)
                }
            }
            Divider()
            ConnectionSummary(store: store)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

@MainActor
struct TrackerDailyTotalText: View {
    @ObservedObject private var task: TrackerTaskDailyTotalStore

    init(totals: TrackerDailyTotalsStore, taskID: String) {
        task = totals.task(taskID)
    }

    var body: some View {
        let text = task.content?.text ?? "-"
        Text(text)
            .font(.callout.monospacedDigit())
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
            .help(task.content?.explanation ?? "Today's total is unavailable")
            .accessibilityLabel("Today's total: \(text)")
    }
}
