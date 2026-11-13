import SwiftUI
import TrackerClient

/// One task in the sidebar: color mark, name and today's total, which appears only once the task has time today.
/// The name stays on one line so that every row has the same height.
struct SidebarTaskRow: View {
    let task: TaskItem
    let isRunning: Bool
    let total: TaskDailyTotal

    private var color: Color { TaskColor.forTaskID(task.id).swiftUIColor }

    /// `00:00:00` is a task without time today. Reading the task's own model keeps a tick from redrawing the list.
    private var trackedToday: Bool {
        guard let text = total.content?.text else { return false }
        return text != "00:00:00" && text != "-"
    }

    var body: some View {
        HStack(spacing: 8) {
            TaskIndicatorView(taskID: task.id, isRunning: isRunning, trackedToday: trackedToday)
                .accessibilityIdentifier(isRunning ? "tracking.active-task" : "task-sidebar.indicator.\(task.id)")
            Text(task.name)
                .fontWeight(isRunning ? .semibold : .regular)
                .foregroundStyle(trackedToday || isRunning ? .primary : .secondary)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 4)
            if trackedToday || isRunning, let content = total.content {
                Text(readableDuration(clockText: content.text))
                    .font(.callout.monospacedDigit())
                    .fontWeight(isRunning ? .semibold : .regular)
                    .foregroundStyle(isRunning ? color : Color.secondary)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                    .help(content.explanation)
                    .accessibilityLabel("Today's total: \(content.text)")
                    .accessibilityIdentifier("task-sidebar.total.\(task.id)")
            }
        }
        .padding(.vertical, 2)
    }
}
