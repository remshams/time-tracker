import SwiftUI

struct WorkLogListView: View {
  var viewModel: WorkLogListViewModel
  let taskID: WorkTask.ID

  var body: some View {
    Table(viewModel.entries) {
      TableColumn(String(localized: "work-log-list.column.time", defaultValue: "Time")) { entry in
        Text(entry.formattedTimeRange)
          .font(.body.monospacedDigit())
          .foregroundStyle(timeRangeForegroundStyle(for: entry))
      }
      TableColumn(String(localized: "work-log-list.column.duration", defaultValue: "Duration")) { entry in
        Text(entry.formattedDuration)
          .font(.body.monospacedDigit())
          .foregroundStyle(.secondary)
      }
      TableColumn(
        String(localized: "work-log-list.column.description", defaultValue: "Description")
      ) { entry in
        Text(entry.description ?? "")
          .foregroundStyle(.secondary)
          .lineLimit(1)
          .truncationMode(.tail)
      }
    }
    .loadingOverlay(
      isLoading: viewModel.isLoading,
      errorTitle: String(
        localized: "work-log-list.error.title",
        defaultValue: "Unable to Load Work Logs"),
      errorMessage: viewModel.errorMessage
    ) {
      if viewModel.isLoaded && viewModel.entries.isEmpty {
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
      }
    }
    .task(id: taskID) {
      await viewModel.loadEntries(for: taskID)
    }
  }

  private func timeRangeForegroundStyle(for entry: WorkLogEntry) -> any ShapeStyle {
    if entry.endedAt == nil {
      return TintShapeStyle()
    }
    return Color.primary
  }
}
