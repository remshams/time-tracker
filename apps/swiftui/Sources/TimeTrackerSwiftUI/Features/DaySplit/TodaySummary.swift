import SwiftUI
import TrackerClient

/// Opens the sidebar with today: the total across all tasks and one bar that splits it by task.
/// The compact form fits on one line for short windows.
struct TodaySummary: View {
    let compact: Bool
    @Environment(DailyTotalsStore.self) private var dailyTotals
    @Environment(TaskCatalogStore.self) private var catalog

    private var slices: [DaySlice] {
        catalog.tasks
            .compactMap { task -> DaySlice? in
                guard let seconds = dailyTotals.taskDurations[task.id], seconds >= 1 else { return nil }
                return DaySlice(task: task, seconds: seconds)
            }
            .sorted { $0.seconds != $1.seconds ? $0.seconds > $1.seconds : $0.task.name < $1.task.name }
    }

    private var title: String { dailyTotals.status == .cached ? "Today, cached" : "Today" }

    var body: some View {
        let slices = slices
        Group {
            if compact {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                    total(slices, size: 20)
                    if !slices.isEmpty {
                        DaySplitBar(slices: slices, height: 8).alignmentGuide(.firstTextBaseline) { $0[.bottom] }
                    }
                }
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                    total(slices, size: 34)
                    if !slices.isEmpty { DaySplitBar(slices: slices, height: 10) }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .help(dailyTotals.totalsExplanation)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("today-summary")
    }

    @ViewBuilder private func total(_ slices: [DaySlice], size: CGFloat) -> some View {
        if let seconds = dailyTotals.dayTotal, seconds >= 1 {
            DurationFigure(text: readableDuration(seconds), size: size, weight: .medium)
                .accessibilityIdentifier("today-summary.total")
        } else {
            Text(dailyTotals.dayTotal == nil ? "Unavailable" : "Nothing tracked yet")
                .font(compact ? .subheadline : .title3)
                .foregroundStyle(.tertiary)
                .accessibilityIdentifier("today-summary.total")
        }
    }
}

struct DaySlice: Identifiable {
    let task: TaskItem
    let seconds: TimeInterval
    var id: String { task.id }
    var color: Color { TaskColor.forTaskID(task.id).swiftUIColor }
}

/// One segment per task with time today, as wide as the task's share. A segment selects its task.
private struct DaySplitBar: View {
    let slices: [DaySlice]
    let height: CGFloat
    @Environment(TaskCatalogStore.self) private var catalog

    private let gap: CGFloat = 2
    private let minimumWidth: CGFloat = 5

    var body: some View {
        GeometryReader { proxy in
            let total = slices.reduce(0) { $0 + $1.seconds }
            let usable = max(0, proxy.size.width - gap * CGFloat(slices.count - 1))
            HStack(spacing: gap) {
                ForEach(slices) { slice in
                    let selected = catalog.selectedTaskID == slice.id
                    Button {
                        if catalog.visibleTasks.contains(where: { $0.id == slice.id }) {
                            catalog.select(slice.id)
                        }
                    } label: {
                        RoundedRectangle(cornerRadius: height / 2.5, style: .continuous)
                            .fill(slice.color)
                            .frame(
                                width: max(minimumWidth, usable * slice.seconds / total),
                                height: selected ? height + 4 : height
                            )
                            .frame(height: height + 4)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("\(slice.task.name), \(readableDuration(slice.seconds))")
                    .accessibilityLabel("\(slice.task.name), \(readableDuration(slice.seconds)) today")
                }
            }
        }
        .frame(height: height + 4)
        .padding(.vertical, -2)
        .accessibilityIdentifier("today-summary.bar")
    }
}
