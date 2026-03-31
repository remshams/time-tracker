import SwiftUI

struct WorkTaskListView: View {
  var viewModel: WorkTaskListViewModel
  @Binding var selection: WorkTask.ID?
  @State private var isAddingTask = false

  var body: some View {
    List(viewModel.tasks, selection: $selection) { task in
      VStack(alignment: .leading, spacing: AppSpacing.tight) {
        Text(task.title)
          .font(.headline)

        if let description = task.description {
          Text(description)
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
      }
      .padding(.vertical, AppSpacing.tight)
      .tag(task.id)
    }
    .listHeader {
      Text(String(localized: "task-list.header.title", defaultValue: "Tasks"))
        .font(.headline)
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, AppSpacing.compact)
        .padding(.vertical, AppSpacing.tight)
    }
    .loadingOverlay(
      isLoading: viewModel.isLoading,
      errorTitle: String(
        localized: "task-list.error.title",
        defaultValue: "Unable to Load Tasks"),
      errorMessage: viewModel.errorMessage
    ) {
      if viewModel.isLoaded && viewModel.tasks.isEmpty {
        PlaceholderView(
          systemImage: "checkmark.circle",
          title: String(
            localized: "task-list.empty.title",
            defaultValue: "No Tasks"),
          description: String(
            localized: "task-list.empty.description",
            defaultValue: "No tasks have been created yet."
          )
        )
      }
    }
    .toolbar {
      ToolbarItem(placement: .primaryAction) {
        Button {
          isAddingTask = true
        } label: {
          Label(
            String(localized: "task-list.add.button", defaultValue: "Add Task"),
            systemImage: "plus")
        }
        .keyboardShortcut("n", modifiers: .command)
        .accessibilityIdentifier("add-task-button")
      }
    }
    .sheet(isPresented: $isAddingTask) {
      NavigationStack {
        AddWorkTaskView { title, description in
          Task {
            await viewModel.createTask(title: title, description: description)
          }
          isAddingTask = false
        }
      }
    }
    .task {
      await viewModel.loadTasks()
    }
  }
}
