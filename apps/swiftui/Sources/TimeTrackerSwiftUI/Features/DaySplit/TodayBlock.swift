import SwiftUI
import TrackerClient

/// Opens the main area with today: the total across all tasks, one bar that splits it by task and a chip for each task.
/// A bar segment or a chip selects its task. The compact form fits on one line and leaves the chips to the bar's hover
/// labels, for short windows.
struct TodayBlock: View {
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
        let selected = slices.first { $0.id == catalog.selectedTaskID }?.id
        Group {
            if compact {
                HStack(alignment: .center, spacing: 10) {
                    heading
                    if !slices.isEmpty {
                        DaySplitBar(slices: slices, selected: selected, height: 8)
                    }
                }
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    heading
                    if !slices.isEmpty {
                        DaySplitBar(slices: slices, selected: selected, height: 10)
                        FlowLayout(spacing: 6, lineSpacing: 6) {
                            ForEach(slices) { slice in
                                DayChip(slice: slice, isSelected: slice.id == selected)
                            }
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .help(dailyTotals.totalsExplanation)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("today-summary")
    }

    private var heading: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(title)
                .font(.system(size: compact ? 13 : 15, weight: .semibold))
                .foregroundStyle(.secondary)
            total
        }
        .fixedSize()
    }

    @ViewBuilder private var total: some View {
        if let seconds = dailyTotals.dayTotal, seconds >= 1 {
            DurationFigure(text: readableDuration(seconds), size: compact ? 20 : 32, weight: .medium)
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

    /// Opens the slice's task when the tab shows it.
    @MainActor func select(in catalog: TaskCatalogStore) {
        if catalog.visibleTasks.contains(where: { $0.id == id }) { catalog.select(id) }
    }
}

/// One segment per task with time today, as wide as the task's share. The selected task's segment gets an outline
/// around it, so the selection never changes a segment's size.
private struct DaySplitBar: View {
    let slices: [DaySlice]
    let selected: String?
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
                    Button {
                        slice.select(in: catalog)
                    } label: {
                        RoundedRectangle(cornerRadius: height / 2.5, style: .continuous)
                            .fill(slice.color)
                            .overlay {
                                if selected == slice.id {
                                    RoundedRectangle(cornerRadius: height / 2.5 + 2, style: .continuous)
                                        .strokeBorder(.primary.opacity(0.6), lineWidth: 1.25)
                                        .padding(-3)
                                }
                            }
                            .frame(width: max(minimumWidth, usable * slice.seconds / total), height: height)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("\(slice.task.name), \(readableDuration(slice.seconds))")
                    .accessibilityLabel("\(slice.task.name), \(readableDuration(slice.seconds)) today")
                }
            }
            .frame(maxHeight: .infinity)
        }
        .frame(height: height + 6)
        .accessibilityIdentifier("today-summary.bar")
    }
}

/// The bar's legend entry: the task's dot, name and time today. The selected task's chip gets an outline in its color.
private struct DayChip: View {
    let slice: DaySlice
    let isSelected: Bool
    @Environment(TaskCatalogStore.self) private var catalog

    var body: some View {
        Button {
            slice.select(in: catalog)
        } label: {
            HStack(spacing: 6) {
                Circle().fill(slice.color).frame(width: 7, height: 7)
                Text(slice.task.name)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Text(readableDuration(slice.seconds))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .fixedSize()
            }
            .font(.subheadline)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(.quaternary.opacity(isSelected ? 1 : 0.55), in: .capsule)
            .overlay { Capsule().strokeBorder(isSelected ? slice.color : .clear, lineWidth: 1.5) }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(slice.task.name), \(readableDuration(slice.seconds)) today")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}
