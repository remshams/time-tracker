import SwiftUI
import TrackerClient

/// The selected task's name on one line, today's total or when it last ran, the auto-pause status, and the Rename and
/// Archive buttons at the trailing edge. Running and idle tasks share one layout.
struct TaskHeader: View {
    let task: TaskItem
    @Environment(TrackingAutomationStore.self) private var automation

    var body: some View {
        HStack(alignment: .top, spacing: Spacing.large) {
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(task.name)
                    .font(TextRole.display)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .textSelection(.enabled)
                    .help(task.name)
                    .accessibilityIdentifier("task-details.name")
                TaskHeaderSubtitleText(task: task)
                if let status = automation.statusText {
                    Text(status)
                        .font(TextRole.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            TaskHeaderActions(task: task)
        }
        .padding(Spacing.xLarge)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("task-details.task.\(task.id)")
    }
}

/// Today's total, or when the task last ran. It reads the task's own total and the day, so a tick redraws only this
/// line.
private struct TaskHeaderSubtitleText: View {
    let task: TaskItem
    @Environment(TrackingStore.self) private var tracking
    @Environment(DailyTotalsStore.self) private var dailyTotals

    var body: some View {
        let subtitle = TaskHeaderSubtitle(
            total: dailyTotals.taskTotal(taskID: task.id).content,
            isRunning: tracking.active?.taskId == task.id,
            lastTracked: dailyTotals.lastTracked(task))
        Group {
            switch subtitle {
            case .today(let duration):
                // The total takes the primary color, like the row's, and its seconds stay dimmed.
                let total = DurationLabel.text(duration)
                    .font(TextRole.duration(.subheadline))
                    .foregroundStyle(.primary)
                Text("\(total) today")
            case .lastTracked(let text):
                Text(text)
            }
        }
        .font(TextRole.detail)
        .foregroundStyle(.secondary)
        .lineLimit(1)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(subtitle.text)
        .accessibilityIdentifier("task-details.subtitle")
    }
}

/// Rename and Archive, or Unarchive, as icon-only buttons.
private struct TaskHeaderActions: View {
    let task: TaskItem
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskArchivingStore.self) private var archiving
    @Environment(TaskRenameStore.self) private var rename

    var body: some View {
        let canChangeArchive =
            task.archived ? archiving.canUnarchive(taskID: task.id) : archiving.canArchive(taskID: task.id)
        HStack(spacing: Spacing.xSmall) {
            Button {
                rename.open(taskID: task.id)
            } label: {
                Label("Edit task name", systemImage: "pencil")
            }
            .disabled(!rename.canOpen)
            .help("Edit task name")

            Button {
                if task.archived {
                    archiving.unarchive(taskID: task.id)
                } else {
                    archiving.openArchive(taskID: task.id)
                }
            } label: {
                Label(
                    task.archived ? "Unarchive task" : "Archive task",
                    systemImage: task.archived ? "archivebox.fill" : "archivebox")
            }
            .disabled(!canChangeArchive)
            .help(
                !task.archived && tracking.active?.taskId == task.id
                    ? "Stop tracking before archiving this task"
                    : task.archived ? "Unarchive this task" : "Archive this task")
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .controlSize(.large)
        .foregroundStyle(.secondary)
        .fixedSize()
    }
}
