import SwiftUI

struct WorkLogListView: View {
    @ObservedObject var viewModel: WorkLogListViewModel
    let taskID: Task.ID

    var body: some View {
        Table(viewModel.entries) {
            TableColumn(String(localized: "work-log-list.column.time", defaultValue: "Time")) { entry in
                Text(entry.formattedTimeRange)
                    .font(.body.monospacedDigit())
            }
            TableColumn(String(localized: "work-log-list.column.duration", defaultValue: "Duration")) { entry in
                Text(entry.formattedDuration)
                    .font(.body.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            TableColumn(String(localized: "work-log-list.column.description", defaultValue: "Description")) { entry in
                Text(entry.description ?? "")
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
        .listLoadingOverlay(
            isLoading: viewModel.isLoading,
            errorTitle: String(
                localized: "work-log-list.error.title",
                defaultValue: "Unable to Load Work Logs"),
            errorMessage: viewModel.errorMessage,
            emptyOverlay: viewModel.isLoaded && viewModel.entries.isEmpty
                ? AnyView(
                    PlaceholderView(
                        systemImage: "clock",
                        title: String(
                            localized: "work-log-list.empty.title",
                            defaultValue: "No Work Logs"),
                        description: String(
                            localized: "work-log-list.empty.description",
                            defaultValue: "No work logs have been recorded for this task yet."
                        )
                    )
                )
                : nil
        )
        .task(id: taskID) {
            await viewModel.loadEntries(for: taskID)
        }
    }
}
