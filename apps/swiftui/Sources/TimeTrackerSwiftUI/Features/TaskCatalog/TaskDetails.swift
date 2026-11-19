import SwiftUI
import TrackerClient

/// The selected task's header and worklogs. Without a selection it shows "Select a task", or the empty state once the
/// tracker confirmed it has no tasks. While the first task list loads, it shows neither.
struct TaskDetails: View {
    @Environment(TaskCatalogStore.self) private var catalog

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let task = catalog.selectedTask {
                TaskHeader(task: task)
                Divider()
                WorklogHistorySection(taskID: task.id)
            } else if catalog.isConfirmedEmpty {
                // A later failed load keeps the empty list, so its error shows above the invitation.
                WorklogHistoryError()
                    .padding([.horizontal, .top], Spacing.xLarge)
                EmptyTasksView()
            } else {
                VStack(alignment: .leading, spacing: Spacing.large) {
                    WorklogHistoryError()
                    if catalog.loaded {
                        Label("Select a task", systemImage: "checklist")
                            .font(TextRole.placeholder)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(Spacing.xLarge)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.background)
    }
}
