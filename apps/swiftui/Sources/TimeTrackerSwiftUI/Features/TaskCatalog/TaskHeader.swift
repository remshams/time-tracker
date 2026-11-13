import SwiftUI
import TrackerClient

/// The selected task's name with its two labelled actions, and one state line while the task runs or is archived.
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
            // A long name takes the full width and the actions drop below it.
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    title(lines: 1)
                    Spacer(minLength: 8)
                    actions
                }
                VStack(alignment: .leading, spacing: 8) {
                    title(lines: 2)
                    actions
                }
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

    private func title(lines: Int) -> some View {
        Text(task.name)
            .font(.system(size: compact ? 20 : 26, weight: .semibold))
            .lineLimit(lines)
            .fixedSize(horizontal: lines == 1, vertical: true)
            .textSelection(.enabled)
            .help(task.name)
            .accessibilityIdentifier("task-details.name")
    }

    private var actions: some View {
        let canChangeArchive =
            task.archived ? archiving.canUnarchive(taskID: task.id) : archiving.canArchive(taskID: task.id)
        return HStack(spacing: 6) {
            Button {
                rename.open(taskID: task.id)
            } label: {
                Label("Rename", systemImage: "pencil")
            }
            .disabled(!rename.canOpen)
            .help("Edit task name")
            .accessibilityLabel("Edit task name")

            Button {
                if task.archived {
                    archiving.unarchive(taskID: task.id)
                } else {
                    archiving.openArchive(taskID: task.id)
                }
            } label: {
                Label(
                    task.archived ? "Unarchive" : "Archive",
                    systemImage: task.archived ? "archivebox.fill" : "archivebox")
            }
            .disabled(!canChangeArchive)
            .help(
                !task.archived && tracking.active?.taskId == task.id
                    ? "Stop tracking before archiving this task"
                    : task.archived ? "Unarchive this task" : "Archive this task")
            .accessibilityLabel(task.archived ? "Unarchive task" : "Archive task")
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.capsule)
        .controlSize(.regular)
        .fixedSize()
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
