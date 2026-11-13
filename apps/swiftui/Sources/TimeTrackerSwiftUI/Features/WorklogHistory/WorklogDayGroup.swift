import SwiftUI
import TrackerClient

/// One day of worklogs: the day with its total, the strip, then one row for each worklog. The total sits in the
/// rows' duration column, over the durations that it adds up.
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
                    .frame(width: HistoryColumns.range, alignment: .leading)
                DayTotal(day: day, taskID: taskID, showsTotal: showsTotal)
                    .frame(width: HistoryColumns.duration, alignment: .trailing)
                Spacer(minLength: 0)
            }
            .padding(.top, 16)
            .accessibilityElement(children: .contain)

            WorklogDayStrip(day: day, axis: axis, color: color, hovered: $hovered)
                .padding(.bottom, 2)

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
