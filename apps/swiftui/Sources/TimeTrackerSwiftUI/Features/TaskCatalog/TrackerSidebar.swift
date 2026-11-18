import SwiftUI
import TrackerClient

/// Today's summary, the Active and Archived switch, and one two-line row for each task in the tab.
struct TrackerSidebar: View {
    let availability: TrackingAvailability
    @Environment(TaskCatalogStore.self) private var catalog

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { catalog.tab }, set: { catalog.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { catalog.selectedTaskID }, set: { catalog.select($0) })
    }

    var body: some View {
        VStack(spacing: 0) {
            if !catalog.tasks.isEmpty {
                TodaySummary()
                    .padding(.horizontal, Spacing.large)
                    .padding(.top, Spacing.xSmall)
                    .padding(.bottom, Spacing.small)
            }

            Picker("Tasks", selection: tabBinding) {
                ForEach(TaskTab.allCases) { tab in
                    Text(tab.rawValue).tag(tab)
                        .accessibilityIdentifier("task-sidebar.tab.\(tab.rawValue)")
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, Spacing.medium)
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

/// One task in the sidebar, always two lines tall: the color mark, the name, and when the task last ran or since when it
/// runs. Today's total sits at the trailing edge once the task has time today. Hovering a row whose task isn't the
/// tracking pill's and can start shows a start button in place of the total, so one task never shows two start or stop
/// controls. The button starts the task and stops the running one. Keyboard and VoiceOver users start a task from the
/// context menu or the row's accessibility action instead.
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
        let total = dailyTotals.taskTotal(taskID: task.id)
        let shownRunning = !connection.stale && isRunning
        HStack(spacing: Spacing.small) {
            TaskRowIndicator(total: total, taskID: task.id, isRunning: shownRunning)
                .accessibilityIdentifier(shownRunning ? "tracking.active-task" : "task-sidebar.indicator.\(task.id)")
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(task.name)
                    .lineLimit(1)
                    .truncationMode(.tail)
                TaskRowSubtitle(task: task)
            }
            .padding(.vertical, Spacing.xSmall)
            Spacer(minLength: Spacing.xSmall)
            ZStack(alignment: .trailing) {
                TrackerDailyTotalText(total: total, taskID: task.id, isRunning: isRunning)
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

/// The row's color mark. It reads the task's total here, so a tick redraws the mark and not the whole row.
private struct TaskRowIndicator: View {
    let total: TaskDailyTotal
    let taskID: String
    let isRunning: Bool

    var body: some View {
        TaskIndicatorView(taskID: taskID, isRunning: isRunning, hasTimeToday: total.content?.hasTime == true)
    }
}

/// When the task last ran, or since when it runs. It reads the task's own values and the day, so a clock tick doesn't
/// redraw it.
private struct TaskRowSubtitle: View {
    let task: TaskItem
    @Environment(TrackingStore.self) private var tracking
    @Environment(DailyTotalsStore.self) private var dailyTotals

    var body: some View {
        Group {
            if let active = tracking.active, active.taskId == task.id {
                Text("Running since \(localTimestamp(active.start))")
            } else {
                Text(dailyTotals.lastTracked(task).text)
            }
        }
        .font(TextRole.caption)
        .foregroundStyle(.secondary)
        .lineLimit(1)
    }
}

/// Today's total while the task has time today or runs. Without a report, or with no time today, the row shows none.
/// It reads only the task's own model, so a tick redraws only the running task's total.
private struct TrackerDailyTotalText: View {
    let total: TaskDailyTotal
    let taskID: String
    let isRunning: Bool

    var body: some View {
        if let content = total.content, let seconds = content.shownSeconds(isRunning: isRunning) {
            let duration = DurationText(seconds: seconds)
            DurationLabel(duration: duration)
                .font(TextRole.duration())
                .foregroundStyle(.primary)
                .lineLimit(1)
                .fixedSize(horizontal: true, vertical: false)
                .help(content.explanation)
                .accessibilityLabel("Today's total: \(duration.hoursAndMinutes)\(duration.seconds)")
                .accessibilityIdentifier("task-sidebar.total.\(taskID)")
        }
    }
}
