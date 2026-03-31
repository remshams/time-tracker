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
    .toolbar {
      ToolbarItem(placement: .primaryAction) {
        trackingToolbarButton
      }
    }
    .alert(
      String(
        localized: "work-log-list.tracking-error.title",
        defaultValue: "Tracking Error"),
      isPresented: Binding(
        get: { viewModel.isShowingTrackingError },
        set: { isPresented in
          if !isPresented {
            viewModel.trackingError = nil
          }
        }
      )
    ) {
      Button(String(localized: "OK", defaultValue: "OK")) {
        viewModel.trackingError = nil
      }
    } message: {
      Text(viewModel.trackingError ?? "")
    }
    .task(id: taskID) {
      await viewModel.loadEntries(for: taskID)
    }
  }

  private var trackingToolbarButton: some View {
    Button {
      if isTrackingSelectedTask {
        Task { await viewModel.stopTracking() }
      } else {
        Task { await viewModel.startTracking(for: taskID) }
      }
    } label: {
      Label(
        isTrackingSelectedTask
          ? String(localized: "work-log-list.toolbar.stop", defaultValue: "Stop Tracking")
          : String(localized: "work-log-list.toolbar.start", defaultValue: "Start Tracking"),
        systemImage: isTrackingSelectedTask ? "stop.fill" : "play.fill"
      )
    }
    .disabled(viewModel.isLoading || viewModel.isTrackingActionInFlight)
  }

  private var isTrackingSelectedTask: Bool {
    viewModel.isTrackingTask(taskID)
  }

  private func timeRangeForegroundStyle(for entry: WorkLogEntry) -> any ShapeStyle {
    if entry.endedAt == nil {
      return TintShapeStyle()
    }
    return Color.primary
  }
}
