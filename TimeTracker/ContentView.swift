import SwiftUI

struct ContentView: View {
  var taskListViewModel: TaskListViewModel
  var workLogListViewModel: WorkLogListViewModel
  @State private var selectedTaskID: WorkTask.ID?

  private var selectedTask: WorkTask? {
    selectedTaskID.flatMap { taskListViewModel.task(for: $0) }
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
            defaultValue: "Choose a task from the list to view its work logs."
          )
        )
        .navigationTitle("")
      }
    }
  }
}
