import SwiftUI
import TrackerClient

/// One task in the sidebar, always two lines tall: color mark, name and when it last ran, and today's total once the
/// task has time today. A hovered row offers its own start button, except for the task the toolbar capsule controls,
/// so one task never shows two start or stop controls.
struct SidebarTaskRow: View {
    let task: TaskItem
    let isRunning: Bool
    let total: TaskDailyTotal
    let availability: TrackingAvailability
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @State private var isHovered = false

    private var color: Color { TaskColor.forTaskID(task.id).swiftUIColor }

    /// `00:00:00` is a task without time today. Reading the task's own model keeps a tick from redrawing the list.
    private var trackedToday: Bool {
        guard let text = total.content?.text else { return false }
        return text != "00:00:00" && text != "-"
    }

    /// The task that the toolbar capsule stops or starts: the running one, else the selected one.
    private var controlledByCapsule: Bool {
        if let active = tracking.active { return active.taskId == task.id }
        return catalog.selectedTaskID == task.id
    }

    var body: some View {
        HStack(spacing: 10) {
            TaskIndicatorView(taskID: task.id, isRunning: isRunning, trackedToday: trackedToday)
                .accessibilityIdentifier(isRunning ? "tracking.active-task" : "task-sidebar.indicator.\(task.id)")
            VStack(alignment: .leading, spacing: 1) {
                Text(task.name)
                    .fontWeight(isRunning ? .semibold : .regular)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Text(isRunning ? "Running" : lastTrackedText(task.latestStart))
                    .font(.caption)
                    .foregroundStyle(isRunning ? color : Color.secondary)
                    .fontWeight(isRunning ? .semibold : .regular)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            if trackedToday || isRunning, let content = total.content {
                Text(readableDuration(clockText: content.text))
                    .font(.callout.monospacedDigit())
                    .foregroundStyle(isRunning ? Color.primary : Color.secondary)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                    .help(content.explanation)
                    .accessibilityLabel("Today's total: \(content.text)")
                    .accessibilityIdentifier("task-sidebar.total.\(task.id)")
            }
            startButton
        }
        .frame(height: 40)
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
    }

    private var startButton: some View {
        let visible = isHovered && !controlledByCapsule && !task.archived
        return Button {
            tracking.startTracking(taskID: task.id)
        } label: {
            Image(systemName: "play.fill")
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(color)
                .frame(width: 22, height: 22)
                .background(color.opacity(0.16), in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .opacity(visible ? 1 : 0)
        .allowsHitTesting(visible)
        .disabled(!availability.canStart(taskID: task.id))
        .help(tracking.active == nil ? "Start tracking" : "Stop the current timer and start this task")
        .accessibilityLabel("Start \(task.name)")
        .accessibilityHidden(controlledByCapsule || task.archived)
        .frame(width: 22)
    }
}
