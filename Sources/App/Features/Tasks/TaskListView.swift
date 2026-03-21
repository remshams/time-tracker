import SwiftUI

struct TaskListView: View {
    @ObservedObject var viewModel: TaskListViewModel
    @Binding var selection: Task.ID?

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
        .listLoadingOverlay(
            isLoading: viewModel.isLoading,
            errorTitle: String(
                localized: "task-list.error.title",
                defaultValue: "Unable to Load Tasks"),
            errorMessage: viewModel.errorMessage
        )
        .task {
            await viewModel.loadTasks()
        }
    }
}
