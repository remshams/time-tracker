import SwiftUI

struct ContentView: View {
    @StateObject private var taskListViewModel: TaskListViewModel
    @StateObject private var workLogListViewModel: WorkLogListViewModel
    @State private var selectedTaskID: Task.ID?

    init(taskListViewModel: TaskListViewModel, workLogListViewModel: WorkLogListViewModel) {
        _taskListViewModel = StateObject(wrappedValue: taskListViewModel)
        _workLogListViewModel = StateObject(wrappedValue: workLogListViewModel)
    }

    var body: some View {
        NavigationSplitView {
            TaskListView(viewModel: taskListViewModel, selection: $selectedTaskID)
        } detail: {
            if let taskID = selectedTaskID {
                WorkLogListView(viewModel: workLogListViewModel, taskID: taskID)
            } else {
                VStack(spacing: AppSpacing.compact) {
                    Image(systemName: "checkmark.circle")
                        .font(.title2)
                    Text(
                        String(
                            localized: "content-view.no-selection.title",
                            defaultValue: "Select a Task")
                    )
                    .font(.headline)
                    Text(
                        String(
                            localized: "content-view.no-selection.description",
                            defaultValue: "Choose a task from the list to view its work logs.")
                    )
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                }
            }
        }
    }
}
