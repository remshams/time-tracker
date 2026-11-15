import SwiftUI
import TrackerClient

struct TrackerSidebar: View {
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(ConnectionStore.self) private var connection
    @Environment(DailyTotalsStore.self) private var dailyTotals

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { catalog.tab }, set: { catalog.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { catalog.selectedTaskID }, set: { catalog.select($0) })
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
            .padding(Spacing.medium)

            HStack {
                Text("Tasks")
                Spacer()
                Text(dailyTotals.status == .cached ? "Today, cached" : "Today")
            }
            .font(TextRole.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, Spacing.large)
            .padding(.bottom, Spacing.xSmall)

            List(selection: selectionBinding) {
                ForEach(catalog.visibleTasks) { task in
                    HStack(spacing: Spacing.small) {
                        TaskIndicatorView(
                            taskID: task.id,
                            isRunning: !connection.stale && tracking.active?.taskId == task.id
                        )
                        .accessibilityIdentifier(
                            !connection.stale && tracking.active?.taskId == task.id
                                ? "tracking.active-task" : "task-sidebar.indicator.\(task.id)")
                        Text(task.name)
                            .lineLimit(2)
                            .padding(.vertical, Spacing.xSmall)
                        Spacer(minLength: Spacing.xSmall)
                        TrackerDailyTotalText(totals: dailyTotals, taskID: task.id)
                    }
                    .tag(task.id)
                    .help(task.name)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("task-sidebar.task.\(task.id)")
                    .contextMenu {
                        TaskContextMenu(
                            taskID: task.id, taskIsArchived: task.archived,
                            taskIsRunning: tracking.active?.taskId == task.id)
                    }
                }
            }
            .listStyle(.sidebar)
            .accessibilityIdentifier("task-sidebar.list")
            .contextMenu {
                TaskContextMenu(taskID: nil)
            }
            .overlay {
                if catalog.visibleTasks.isEmpty {
                    VStack(spacing: Spacing.small) {
                        Image(systemName: "tray")
                        Text("No \(catalog.tab.rawValue.lowercased()) tasks")
                            .accessibilityIdentifier("task-sidebar.empty")
                    }
                    .foregroundStyle(.secondary)
                }
            }
            Divider()
            ConnectionSummary()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct TrackerDailyTotalText: View {
    private let task: TaskDailyTotal
    private let taskID: String

    init(totals: DailyTotalsStore, taskID: String) {
        self.taskID = taskID
        task = totals.taskTotal(taskID: taskID)
    }

    var body: some View {
        let text = task.content?.text ?? "-"
        Text(text)
            .font(TextRole.duration())
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: false)
            .help(task.content?.explanation ?? "Today's total is unavailable")
            .accessibilityLabel("Today's total: \(text)")
            .accessibilityIdentifier("task-sidebar.total.\(taskID)")
    }
}
