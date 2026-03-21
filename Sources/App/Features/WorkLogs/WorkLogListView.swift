import SwiftUI

struct WorkLogListView: View {
    @ObservedObject var viewModel: WorkLogListViewModel
    let taskID: Task.ID

    var body: some View {
        List(viewModel.entries) { entry in
            WorkLogRowView(entry: entry)
        }
        .overlay {
            if viewModel.isLoading {
                ProgressView()
            } else if let errorMessage = viewModel.errorMessage {
                VStack(spacing: AppSpacing.compact) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.title2)
                    Text(
                        String(
                            localized: "work-log-list.error.title",
                            defaultValue: "Unable to Load Work Logs")
                    )
                    .font(.headline)
                    Text(errorMessage)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            } else if viewModel.entries.isEmpty {
                VStack(spacing: AppSpacing.compact) {
                    Image(systemName: "clock")
                        .font(.title2)
                    Text(
                        String(
                            localized: "work-log-list.empty.title",
                            defaultValue: "No Work Logs")
                    )
                    .font(.headline)
                    Text(
                        String(
                            localized: "work-log-list.empty.description",
                            defaultValue: "No work logs have been recorded for this task yet.")
                    )
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                }
            }
        }
        .task(id: taskID) {
            await viewModel.loadEntries(for: taskID)
        }
    }
}
