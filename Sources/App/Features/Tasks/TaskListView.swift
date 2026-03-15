import SwiftUI

struct TaskListView: View {
    @ObservedObject var viewModel: TaskListViewModel

    var body: some View {
        List(viewModel.tasks) { task in
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
        }
        .overlay {
            if viewModel.isLoading {
                ProgressView()
            } else if let errorMessage = viewModel.errorMessage {
                VStack(spacing: AppSpacing.compact) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.title2)
                    Text("Unable to Load Tasks")
                        .font(.headline)
                    Text(errorMessage)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .task {
            await viewModel.loadTasks()
        }
    }
}
