import SwiftUI
import TrackerClient

/// The main area: today's split on top, then the selected task's title and its worklogs. The content sits in one
/// column of limited width, so rows stay readable in a wide window.
struct TaskDetails: View {
    @Environment(TaskCatalogStore.self) private var catalog
    @State private var compact = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: compact ? 10 : 18) {
                TodayBlock(compact: compact)
                if let task = catalog.selectedTask {
                    TaskHeader(task: task, compact: compact)
                }
            }
            .padding(.horizontal, HistoryColumns.gutter)
            .padding(.top, compact ? 4 : 14)
            .padding(.bottom, compact ? 8 : 12)
            .readingColumn()

            if let task = catalog.selectedTask {
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

/// Fills the area below today's split while no task is selected, and shows a failed history load.
private struct NoTaskSelected: View {
    @Environment(TaskCatalogStore.self) private var catalog

    var body: some View {
        VStack(spacing: 10) {
            WorklogHistoryError()
            if !catalog.visibleTasks.isEmpty {
                Image(systemName: "list.bullet.rectangle")
                    .font(.system(size: 30, weight: .light))
                    .foregroundStyle(.tertiary)
                Text("Select a task to see its worklogs.")
                    .foregroundStyle(.secondary)
            }
        }
        .multilineTextAlignment(.center)
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
