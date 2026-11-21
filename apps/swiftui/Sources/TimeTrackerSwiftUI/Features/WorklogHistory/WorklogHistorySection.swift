import SwiftUI
import TrackerClient

/// Shows the selected task's worklogs by day, with their loading, empty, error and pagination states.
struct WorklogHistorySection: View {
    let taskID: String
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            WorklogHistoryError()
                .padding([.horizontal, .top], Spacing.xLarge)
                .readingColumn()
            if history.worklogs.isEmpty {
                Group {
                    if history.isLoading {
                        DelayedProgressIndicator(
                            isBusy: history.isLoading, title: "Loading worklogs...",
                            identifier: "worklog-history.loading"
                        )
                    } else if history.error == nil {
                        Label("No worklogs yet", systemImage: "clock")
                            .accessibilityIdentifier("worklog-history.empty")
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(Spacing.xLarge)
                .readingColumn()
            } else {
                WorklogDayList(taskID: taskID)
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
    }
}

/// The hour labels over the scrolling day groups. Reading the worklog days here redraws the list each second while a
/// worklog runs, because its row's duration and bar grow.
private struct WorklogDayList: View {
    let taskID: String
    @Environment(WorklogHistoryStore.self) private var history
    @Environment(TrackingStore.self) private var tracking
    @Environment(DailyTotalsStore.self) private var dailyTotals

    var body: some View {
        let shown = WorklogDays(history: history, tracking: tracking, dailyTotals: dailyTotals, taskID: taskID)
        let color = ColorRole.task(TaskColor.forTaskID(taskID))
        VStack(spacing: 0) {
            WorklogHourLabels(axis: shown.axis)
                .padding(.horizontal, HistoryColumns.gutter)
                .padding(.vertical, Spacing.xSmall)
                .readingColumn()
            Divider()
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(shown.days) { day in
                        WorklogDayGroup(day: day, axis: shown.axis, color: color)
                    }
                    if history.hasMore {
                        HistoryPaginationButton()
                            .padding(.top, Spacing.medium)
                    }
                }
                .padding(.vertical, Spacing.small)
                .padding(.horizontal, HistoryColumns.gutter)
                .readingColumn()
            }
            .accessibilityIdentifier("worklog-history.scroll")
        }
    }
}

/// One day: its title and total, then one row for each worklog. The total sits in the rows' duration column, over the
/// durations it adds up.
private struct WorklogDayGroup: View {
    let day: WorklogDay
    let axis: WorklogHourAxis
    let color: Color

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: HistoryColumns.spacing) {
                Text(day.title)
                    .font(TextRole.detailEmphasis)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Group {
                    if let total = day.total {
                        DurationLabel(duration: total)
                            .font(TextRole.duration(.subheadline))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                .frame(width: HistoryColumns.duration, alignment: .trailing)
            }
            .padding(.top, Spacing.medium)
            .padding(.bottom, Spacing.xxSmall)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(day.accessibilityLabel)
            .accessibilityAddTraits(.isHeader)
            .accessibilityIdentifier("worklog-history.day.\(day.title)")

            ForEach(day.rows) { row in
                WorklogRow(row: row, axis: axis, color: color)
            }
        }
    }
}

/// Shows the history panel's message, with a retry button when the history failed to load.
struct WorklogHistoryError: View {
    @Environment(WorklogHistoryStore.self) private var history

    var body: some View {
        if let error = history.error {
            VStack(alignment: .leading, spacing: Spacing.small) {
                Label {
                    Text(error).textSelection(.enabled)
                        .accessibilityIdentifier("worklog-history.error")
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(ColorRole.error)
                }
                .foregroundStyle(.primary)
                if history.unavailable {
                    Button("Retry") { history.reload() }
                        .accessibilityIdentifier("worklog-history.retry")
                }
            }
            .padding(Spacing.medium)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: CornerRadius.medium))
            .overlay {
                RoundedRectangle(cornerRadius: CornerRadius.medium)
                    .stroke(.quaternary, lineWidth: BorderWidth.hairline)
            }
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
