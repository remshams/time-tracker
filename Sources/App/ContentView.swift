import SwiftUI

struct ContentView: View {
    @StateObject private var taskListViewModel: TaskListViewModel
    @StateObject private var workLogListViewModel: WorkLogListViewModel
    @State private var selectedTaskID: Task.ID?

    private var selectedTask: Task? {
        taskListViewModel.tasks.first { $0.id == selectedTaskID }
    }

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
                    .navigationTitle(selectedTask?.title ?? "")
            } else {
                PlaceholderView(
                    systemImage: "checkmark.circle",
                    title: String(
                        localized: "content-view.no-selection.title",
                        defaultValue: "Select a Task"),
                    description: String(
                        localized: "content-view.no-selection.description",
                        defaultValue: "Choose a task from the list to view its work logs."))
                .navigationTitle("")
            }
        }
    }
}
