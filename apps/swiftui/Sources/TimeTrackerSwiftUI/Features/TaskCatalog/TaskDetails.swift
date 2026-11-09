import SwiftUI
import TrackerClient

struct TaskDetails: View {
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(TrackingAutomationStore.self) private var automation

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let task = catalog.selectedTask {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .top, spacing: 16) {
                        Text(task.name)
                            .font(.system(size: 26, weight: .semibold))
                            .foregroundStyle(.primary)
                            .lineLimit(3)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                            .help(task.name)
                            .accessibilityIdentifier("task-details.name")
                    }

                    HStack(spacing: 12) {
                        if task.archived {
                            Label("Archived", systemImage: "archivebox")
                                .accessibilityIdentifier("task-details.state")
                                .foregroundStyle(.secondary)
                        } else if let active = tracking.active, active.taskId == task.id {
                            Label("Running since \(localTimestamp(active.start))", systemImage: "timer")
                                .accessibilityIdentifier("task-details.state")
                                .foregroundStyle(.secondary)
                        } else {
                            Label("Active", systemImage: "checklist")
                                .accessibilityIdentifier("task-details.state")
                                .foregroundStyle(.secondary)
                        }

                        if let status = automation.statusText {
                            Text(status)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(24)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("task-details.task.\(task.id)")

                Divider()
                WorklogHistorySection()
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    WorklogHistoryError()
                    Label("Select a task", systemImage: "checklist")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                }
                .padding(24)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.background)
    }
}
