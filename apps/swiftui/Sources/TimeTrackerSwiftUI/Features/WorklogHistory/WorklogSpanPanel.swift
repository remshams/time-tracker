import SwiftUI
import TrackerClient

/// One worklog the way the history shows it, for a dialog that changes, moves or removes it: the day and the time
/// range, the duration as the main figure, and the worklog's bar in the task's color on the hours around it. A running
/// worklog's duration and bar grow with the tracking clock, as in the history, and an end before the start shows a
/// dash.
///
/// The hours cover both the times the dialog opened with and the current ones, so editing a time moves the bar and
/// keeps the scale.
struct WorklogSpanPanel: View {
    let start: Date
    /// `nil` while the worklog runs.
    let end: Date?
    let taskID: String
    var durationIdentifier = ""
    @Environment(TrackingStore.self) private var tracking
    @Environment(\.timeZone) private var timeZone
    @State private var opened: WorklogSpan?

    var body: some View {
        // Reading the running end only for a running worklog keeps a finished one from redrawing every second. While
        // the elapsed time is unknown, a running worklog is a moment at its start and its duration is a dash.
        let now = end == nil ? tracking.runningEnd ?? start : .now
        let span = WorklogSpan(start: start, end: end, now: now, calendar: calendar)
        let axis = WorklogHourAxis(covering: [span, opened ?? span])
        let color = ColorRole.task(TaskColor.forTaskID(taskID))
        VStack(alignment: .leading, spacing: Spacing.small) {
            // The last baselines line up, so the large duration sits level with the time range.
            HStack(alignment: .lastTextBaseline, spacing: Spacing.medium) {
                VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                    Text(span.day)
                        .font(TextRole.heading)
                    Text(span.range)
                        .font(TextRole.detail)
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: Spacing.medium)
                duration(until: end ?? now)
                    .font(TextRole.dayTotal)
            }
            VStack(spacing: 0) {
                WorklogBar(
                    startHour: span.startHour, endHour: span.endHour, isRunning: end == nil, axis: axis,
                    color: color, highlighted: true)
                WorklogHourScale(axis: axis)
            }
            // A running bar grows every second, which needs no animation. An edited time animates.
            .animation(end == nil ? nil : Motion.standard, value: span)
        }
        .onAppear {
            if opened == nil { opened = span }
        }
        .padding(.vertical, Spacing.small)
    }

    @ViewBuilder
    private func duration(until end: Date) -> some View {
        if end > start {
            // No accessibility label: the UI tests read the shown duration as the element's value.
            DurationLabel(duration: DurationText(duration: end.timeIntervalSince(start)))
                .accessibilityIdentifier(durationIdentifier)
        } else {
            Text("–").foregroundStyle(.secondary)
        }
    }

    private var calendar: Calendar {
        var calendar = Calendar.autoupdatingCurrent
        calendar.timeZone = timeZone
        return calendar
    }
}
