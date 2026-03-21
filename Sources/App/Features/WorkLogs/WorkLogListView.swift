import SwiftUI

struct WorkLogListView: View {
    @ObservedObject var viewModel: WorkLogListViewModel
    let taskID: Task.ID

    var body: some View {
        List(viewModel.entries) { entry in
            WorkLogRowView(entry: entry)
        }
        .listHeader {
            VStack(spacing: 0) {
                HStack(alignment: .firstTextBaseline, spacing: AppSpacing.compact) {
                    Text(String(localized: "work-log-list.header.time", defaultValue: "Time"))
                        .frame(width: WorkLogColumnWidths.time, alignment: .leading)
                    Text(String(localized: "work-log-list.header.duration", defaultValue: "Duration"))
                        .frame(width: WorkLogColumnWidths.duration, alignment: .leading)
                    Text(String(localized: "work-log-list.header.description", defaultValue: "Description"))
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .padding(.horizontal, AppSpacing.compact)
                .padding(.vertical, AppSpacing.tight)
                Divider()
            }
        }
        .listLoadingOverlay(
            isLoading: viewModel.isLoading,
            errorTitle: String(
                localized: "work-log-list.error.title",
                defaultValue: "Unable to Load Work Logs"),
            errorMessage: viewModel.errorMessage,
            emptyOverlay: AnyView(
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
        )
        .task(id: taskID) {
            await viewModel.loadEntries(for: taskID)
        }
    }
}
