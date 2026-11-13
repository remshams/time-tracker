import SwiftUI
import TrackerClient

/// The task list: the Active and Archived switch, one two-line row for each task, and the list's own actions.
struct TrackerSidebar: View {
    let availability: TrackingAvailability
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
            .padding(.horizontal, 14)
            .padding(.top, 4)
            .padding(.bottom, 8)

            List(selection: selectionBinding) {
                ForEach(catalog.visibleTasks) { task in
                    let isRunning = !connection.stale && tracking.active?.taskId == task.id
                    SidebarTaskRow(
                        task: task, isRunning: isRunning, total: dailyTotals.taskTotal(taskID: task.id),
                        availability: availability
                    )
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
                    VStack(spacing: 8) {
                        Image(systemName: catalog.tab == .active ? "checklist" : "archivebox")
                            .font(.system(size: 24, weight: .light))
                        Text("No \(catalog.tab.rawValue.lowercased()) tasks")
                            .accessibilityIdentifier("task-sidebar.empty")
                    }
                    .foregroundStyle(.secondary)
                }
            }
            Divider()
            HStack(spacing: 2) {
                TrackerSidebarToolbar()
                ConnectionSummary()
            }
            .padding(.leading, 8)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
