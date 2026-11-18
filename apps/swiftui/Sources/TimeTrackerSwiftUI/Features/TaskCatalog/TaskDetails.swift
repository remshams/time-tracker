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
                WorklogHistorySection()
            } else if catalog.isConfirmedEmpty {
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
