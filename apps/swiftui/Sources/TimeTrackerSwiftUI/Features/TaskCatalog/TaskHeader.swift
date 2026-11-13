import SwiftUI
import TrackerClient

/// The selected task's name with the actions that belong to it, and one state line while the task runs or is archived.
/// Start and stop live in the toolbar capsule only.
struct TaskHeader: View {
    let task: TaskItem
    let compact: Bool
    @Environment(TrackingStore.self) private var tracking
    @Environment(TrackingAutomationStore.self) private var automation
    @Environment(TaskArchivingStore.self) private var archiving
    @Environment(TaskRenameStore.self) private var rename

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                Text(task.name)
                    .font(.system(size: compact ? 19 : 24, weight: .semibold))
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .help(task.name)
                    .accessibilityIdentifier("task-details.name")
                Spacer(minLength: 8)
                actions
            }
            stateLine
            if let status = automation.statusText {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("task-details.task.\(task.id)")
    }

    private var actions: some View {
        let canChangeArchive =
            task.archived ? archiving.canUnarchive(taskID: task.id) : archiving.canArchive(taskID: task.id)
        return HStack(spacing: 2) {
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
        .buttonStyle(.borderless)
        .labelStyle(.iconOnly)
        .font(.system(size: 15))
        .controlSize(.large)
        .foregroundStyle(.secondary)
    }

    @ViewBuilder private var stateLine: some View {
        if task.archived {
            Label("Archived", systemImage: "archivebox")
                .font(.callout)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("task-details.state")
        } else if let active = tracking.active, active.taskId == task.id {
            Text("Running since \(localTimestamp(active.start))")
                .font(.callout)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("task-details.state")
        }
    }
}
