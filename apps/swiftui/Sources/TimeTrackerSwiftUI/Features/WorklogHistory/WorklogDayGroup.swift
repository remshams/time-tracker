import SwiftUI
import TrackerClient

/// One day of worklogs: a line with the day, its strip and its total, then one row for each worklog.
struct WorklogDayGroup: View {
    let day: WorklogDay
    let axis: TimeAxis
    let color: Color
    let active: WorklogItem?
    @Binding var hovered: String?
    let showsTotal: Bool
    let taskID: String

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: HistoryColumns.spacing) {
                Text(day.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                    .frame(width: HistoryColumns.label, alignment: .leading)
                WorklogDayStrip(day: day, axis: axis, color: color, hovered: $hovered)
                DayTotal(day: day, taskID: taskID, showsTotal: showsTotal)
                    .frame(width: HistoryColumns.total, alignment: .trailing)
                Color.clear.frame(width: HistoryColumns.actions, height: 1)
            }
            .padding(.horizontal, 8)
            .padding(.top, 14)
            .padding(.bottom, 4)
            .accessibilityElement(children: .contain)

            ForEach(day.segments) { segment in
                WorklogRow(segment: segment, color: color, active: active, hovered: $hovered)
            }
        }
    }
}

/// A day's total. Today's total comes from the daily totals, which also count the running worklog.
/// The oldest loaded day hides its total while older worklogs are still unloaded, because the sum would be partial.
private struct DayTotal: View {
    let day: WorklogDay
    let taskID: String
    let showsTotal: Bool
    @Environment(DailyTotalsStore.self) private var dailyTotals

    var body: some View {
        if showsTotal {
            Text(text)
                .font(.subheadline.monospacedDigit())
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }

    private var text: String {
        if day.isToday, let stored = dailyTotals.taskTotal(taskID: taskID).content?.text, stored != "-" {
            return readableDuration(clockText: stored)
        }
        return readableDuration(day.seconds)
    }
}
