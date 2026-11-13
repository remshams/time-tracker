import SwiftUI
import TrackerClient

struct TrackerSidebar: View {
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(ConnectionStore.self) private var connection
    @Environment(DailyTotalsStore.self) private var dailyTotals
    @State private var compact = false

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { catalog.tab }, set: { catalog.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { catalog.selectedTaskID }, set: { catalog.select($0) })
    }

    var body: some View {
        VStack(spacing: 0) {
            TodaySummary(compact: compact)
                .padding(.horizontal, 16)
                .padding(.top, 4)
                .padding(.bottom, compact ? 8 : 12)

            Picker("Tasks", selection: tabBinding) {
                ForEach(TaskTab.allCases) { tab in
                    Text(tab.rawValue).tag(tab)
                        .accessibilityIdentifier("task-sidebar.tab.\(tab.rawValue)")
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 12)
            .padding(.bottom, 6)

            List(selection: selectionBinding) {
                ForEach(catalog.visibleTasks) { task in
                    let isRunning = !connection.stale && tracking.active?.taskId == task.id
                    SidebarTaskRow(task: task, isRunning: isRunning, total: dailyTotals.taskTotal(taskID: task.id))
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
        .onGeometryChange(for: Bool.self) {
            $0.size.height < 600
        } action: {
            compact = $0
        }
    }
}
