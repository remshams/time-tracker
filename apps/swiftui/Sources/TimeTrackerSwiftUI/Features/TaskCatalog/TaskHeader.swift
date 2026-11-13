import SwiftUI
import TrackerClient

/// The selected task's name, one state line, its timer and the one start or stop button.
/// The timer and the button share a baseline, and the header carries the task color only while the task runs.
struct TaskHeader: View {
    let task: TaskItem
    let availability: TrackingAvailability
    let compact: Bool
    @Environment(TrackingStore.self) private var tracking
    @Environment(TrackingAutomationStore.self) private var automation
    @Environment(WorklogHistoryStore.self) private var history
    @Environment(DailyTotalsStore.self) private var dailyTotals

    private var color: Color { TaskColor.forTaskID(task.id).swiftUIColor }
    private var isRunning: Bool { tracking.active?.taskId == task.id }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 10 : 14) {
            VStack(alignment: .leading, spacing: 3) {
                Text(task.name)
                    .font(.system(size: compact ? 24 : 28, weight: .semibold))
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .help(task.name)
                    .accessibilityIdentifier("task-details.name")
                stateLine
                if let status = automation.statusText {
                    Text(status)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if !task.archived {
                TrackingRow(task: task, availability: availability, compact: compact)
            }
        }
        .padding(.horizontal, 24)
        .padding(.top, compact ? 10 : 14)
        .padding(.bottom, compact ? 14 : 20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("task-details.task.\(task.id)")
    }

    @ViewBuilder private var stateLine: some View {
        Group {
            if task.archived {
                Label("Archived", systemImage: "archivebox")
            } else if let active = tracking.active, active.taskId == task.id {
                Text("Running since \(localTimestamp(active.start))")
            } else {
                Text(idleState)
            }
        }
        .font(.callout)
        .foregroundStyle(.secondary)
        .accessibilityIdentifier("task-details.state")
    }

    private var idleState: String {
        if dailyTotals.taskDurations[task.id, default: 0] >= 1 { return "Today so far" }
        if let end = history.worklogs.lazy.compactMap({ parseTimestamp($0.end) }).first {
            return lastWorkedText(end: end)
        }
        return history.worklogs.isEmpty && !history.isLoading && history.error == nil
            ? "No time tracked yet" : " "
    }
}

/// The task's timer and its start or stop button. With no time to show, the button stands alone.
private struct TrackingRow: View {
    let task: TaskItem
    let availability: TrackingAvailability
    let compact: Bool
    @Environment(TrackingStore.self) private var tracking
    @Environment(DailyTotalsStore.self) private var dailyTotals

    private var color: Color { TaskColor.forTaskID(task.id).swiftUIColor }
    private var size: CGFloat { compact ? 44 : 58 }
    /// Keeps the button in one place while a stopped task starts and its figure changes.
    private var figureWidth: CGFloat { size * 4.3 }

    /// Today's total while the task is not running. It stays `nil` while the task has no time today.
    private var todayText: String? {
        guard tracking.active?.taskId != task.id,
            let text = dailyTotals.taskTotal(taskID: task.id).content?.text, text != "00:00:00", text != "-"
        else { return nil }
        return readableDuration(clockText: text)
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            if tracking.active?.taskId == task.id {
                RunningFigure(size: size, color: color)
                    .frame(minWidth: figureWidth, alignment: .leading)
            } else if let todayText {
                DurationFigure(text: todayText, size: size)
                    .foregroundStyle(.secondary)
                    .frame(minWidth: figureWidth, alignment: .leading)
                    .accessibilityIdentifier("task-details.today")
            }
            TrackingToggleButton(taskID: task.id, availability: availability)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// The running worklog's elapsed time. Only this view redraws on each tick.
private struct RunningFigure: View {
    let size: CGFloat
    let color: Color
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        DurationFigure(text: readableDuration(clockText: tracking.timerText), size: size)
            .foregroundStyle(color)
            .accessibilityIdentifier("task-details.timer")
    }
}

/// Says when the task was last worked on, in words a person would use.
func lastWorkedText(end: Date) -> String {
    let calendar = Calendar.current
    let time = end.formatted(date: .omitted, time: .shortened)
    if calendar.isDateInToday(end) { return "Last worked today, \(time)" }
    if calendar.isDateInYesterday(end) { return "Last worked yesterday, \(time)" }
    return "Last worked \(end.formatted(date: .abbreviated, time: .omitted))"
}
