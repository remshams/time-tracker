import SwiftUI
import TrackerClient

struct TaskDetails: View {
    let availability: TrackingAvailability
    @Environment(TaskCatalogStore.self) private var catalog
    @State private var compact = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let task = catalog.selectedTask {
                TaskHeader(task: task, availability: availability, compact: compact)
                WorklogHistorySection(taskID: task.id)
            } else {
                NoTaskSelected()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(.background)
        .onGeometryChange(for: Bool.self) {
            $0.size.height < 560
        } action: {
            compact = $0
        }
    }
}

/// Fills the pane while no task is selected. With no tasks at all it points to creating the first one.
private struct NoTaskSelected: View {
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(TaskCreationStore.self) private var creation

    var body: some View {
        VStack(spacing: 14) {
            WorklogHistoryError()
            Image(systemName: "checklist")
                .font(.system(size: 34, weight: .light))
                .foregroundStyle(.tertiary)
            VStack(spacing: 4) {
                Text("Select a task")
                    .font(.title3.weight(.semibold))
                Text(
                    catalog.visibleTasks.isEmpty
                        ? "You have no \(catalog.tab.rawValue.lowercased()) tasks yet."
                        : "Pick one from the list to track time or review its worklogs."
                )
                .foregroundStyle(.secondary)
            }
            if catalog.visibleTasks.isEmpty && catalog.tab == .active {
                Button("Create a task") { creation.open() }
                    .buttonStyle(.glassProminent)
                    .controlSize(.large)
                    .disabled(!creation.canOpen)
                    .accessibilityIdentifier("task-details.create")
            }
        }
        .multilineTextAlignment(.center)
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
