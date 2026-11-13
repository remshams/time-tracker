import SwiftUI
import TrackerClient

/// Shows the selected task's worklogs by day, with their loading, empty, error and pagination states.
struct WorklogHistorySection: View {
    let taskID: String
    @Environment(TrackingStore.self) private var tracking
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        VStack(spacing: 0) {
            WorklogHistoryError()
                .padding(.horizontal, HistoryColumns.gutter)
                .padding(.bottom, 12)
                .readingColumn()
            if history.worklogs.isEmpty {
                if history.isLoading {
                    DelayedProgressIndicator(
                        isBusy: history.isLoading, title: "Loading worklogs...",
                        identifier: "worklog-history.loading"
                    )
                    .padding(.vertical, 24)
                } else if history.error == nil {
                    NoWorklogs(taskID: taskID)
                }
                Spacer(minLength: 0)
            } else {
                RunningMinute { runningEnd in
                    WorklogDays(taskID: taskID, runningEnd: runningEnd)
                }
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .accessibilityElement(children: .contain)
    }
}

/// Passes the running worklog's end to its content, rounded down to the minute. The strips don't need finer ends, so
/// the content redraws once a minute instead of on every tick.
private struct RunningMinute<Content: View>: View {
    @ViewBuilder let content: (Date?) -> Content
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        content(runningEnd)
    }

    private var runningEnd: Date? {
        guard let active = tracking.active, let start = parseTimestamp(active.start),
            let elapsed = clockSeconds(tracking.timerText)
        else { return nil }
        return start.addingTimeInterval((elapsed / 60).rounded(.down) * 60)
    }
}

private struct WorklogDays: View {
    let taskID: String
    let runningEnd: Date?
    @Environment(TrackingStore.self) private var tracking
    @Environment(WorklogHistoryStore.self) private var history

    private var color: Color { TaskColor.forTaskID(taskID).swiftUIColor }

    var body: some View {
        let days = WorklogDay.grouping(history.worklogs, runningEnd: runningEnd)
        let axis = TimeAxis(days: days)
        VStack(spacing: 0) {
            VStack(spacing: 0) {
                TimeAxisLabels(axis: axis)
                    .padding(.horizontal, HistoryColumns.gutter)
                    .padding(.top, 6)
                    .padding(.bottom, 4)
                    .readingColumn()
                Divider()
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(days.enumerated()), id: \.element.id) { index, day in
                        WorklogDayGroup(
                            day: day, axis: axis, color: color, active: tracking.active,
                            showsTotal: !(history.hasMore && index == days.count - 1), taskID: taskID)
                    }
                    if history.hasMore {
                        HistoryPaginationButton()
                            .padding(.top, 14)
                    }
                }
                .padding(.vertical, 8)
                .padding(.horizontal, HistoryColumns.gutter)
                .readingColumn()
            }
            .accessibilityIdentifier("worklog-history.scroll")
        }
    }
}

/// Tells an empty history what to do next instead of leaving a blank pane. The toolbar capsule is the one control that
/// starts a task, so an active task points to it. An archived task offers to come back.
private struct NoWorklogs: View {
    let taskID: String
    @Environment(TaskCatalogStore.self) private var catalog

    private var archived: Bool { catalog.selectedTask?.archived ?? false }

    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: "clock")
                .font(.system(size: 28, weight: .light))
                .foregroundStyle(.tertiary)
                .padding(.bottom, 4)
            Text("No worklogs yet")
                .font(.headline)
                .accessibilityIdentifier("worklog-history.empty")
            Text(
                archived
                    ? "Unarchive the task to track time on it."
                    : "Start tracking from the toolbar. Each stretch of work shows up here as a bar and a row."
            )
            .foregroundStyle(.secondary)
            .frame(maxWidth: 320)
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
        .padding(.top, 48)
        .padding(.horizontal, 24)
    }
}

/// Shows the history panel's message, with a retry button when the history failed to load.
struct WorklogHistoryError: View {
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        if let error = history.error {
            VStack(alignment: .leading, spacing: 8) {
                Label {
                    Text(error).textSelection(.enabled)
                        .accessibilityIdentifier("worklog-history.error")
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                }
                .foregroundStyle(.primary)
                if history.unavailable {
                    Button("Retry") { history.reload() }
                        .accessibilityIdentifier("worklog-history.retry")
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct HistoryPaginationButton: View {
    @Environment(WorklogHistoryStore.self) private var history
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        Button("Load older worklogs") { history.loadOlder() }
            .accessibilityIdentifier("worklog-history.load-older")
            .disabled(connection.stale || history.isLoading)
    }
}
