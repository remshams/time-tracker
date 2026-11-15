import SwiftUI
import TrackerClient

struct TrackerSidebar: View {
    let availability: TrackingAvailability
    @Environment(TaskCatalogStore.self) private var catalog
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
                    TrackerSidebarRow(task: task, availability: availability)
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

/// One task in the sidebar. Hovering a row whose task isn't the tracking pill's and can start shows a start button in
/// place of today's total, so one task never shows two start or stop controls. The button starts the task and stops
/// the running one. Keyboard and VoiceOver users start a task from the context menu or the row's accessibility action instead.
private struct TrackerSidebarRow: View {
    let task: TaskItem
    let availability: TrackingAvailability
    @Environment(TrackingStore.self) private var tracking
    @Environment(TrackingPill.self) private var pill
    @Environment(ConnectionStore.self) private var connection
    @Environment(DailyTotalsStore.self) private var dailyTotals
    @State private var isHovered = false

    private var isRunning: Bool { tracking.active?.taskId == task.id }
    private var canStart: Bool { availability.canStart(taskID: task.id) }
    private var showsStart: Bool { isHovered && pill.taskID != task.id && canStart }

    var body: some View {
        HStack(spacing: Spacing.small) {
            TaskIndicatorView(taskID: task.id, isRunning: !connection.stale && isRunning)
                .accessibilityIdentifier(
                    !connection.stale && isRunning ? "tracking.active-task" : "task-sidebar.indicator.\(task.id)")
            Text(task.name)
                .lineLimit(2)
                .padding(.vertical, Spacing.xSmall)
            Spacer(minLength: Spacing.xSmall)
            ZStack(alignment: .trailing) {
                TrackerDailyTotalText(totals: dailyTotals, taskID: task.id)
                    .opacity(showsStart ? 0 : 1)
                startButton
            }
            .animation(Motion.standard, value: showsStart)
        }
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .tag(task.id)
        .help(task.name)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("task-sidebar.task.\(task.id)")
        .accessibilityActions {
            if !task.archived && !isRunning {
                Button("Start \(task.name)") { tracking.startTracking(taskID: task.id) }
                    .disabled(!canStart)
            }
        }
        .contextMenu {
            TaskContextMenu(
                taskID: task.id, taskIsArchived: task.archived, taskIsRunning: isRunning, canStartTracking: canStart)
        }
    }

    private var startButton: some View {
        let color = ColorRole.task(TaskColor.forTaskID(task.id))
        return Button {
            tracking.startTracking(taskID: task.id)
        } label: {
            Image(systemName: "play.fill")
                .font(TextRole.actionGlyph)
                .foregroundStyle(color)
                .frame(width: IconSize.button, height: IconSize.button)
                .background(color.opacity(Emphasis.hoverTint), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .opacity(showsStart ? 1 : 0)
        .allowsHitTesting(showsStart)
        // Hidden, the button must not take keyboard focus and start a task nobody sees.
        .disabled(!showsStart)
        .help(tracking.active == nil ? "Start tracking \(task.name)" : "Stop the current timer and start \(task.name)")
        .accessibilityHidden(true)
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
