import SwiftUI
import TrackerClient

/// Heads the task list with today: the total across all tasks, one bar that splits it by task and one readout line.
/// The readout names the selected task and its share, and follows the pointer while it rests on another segment.
/// A segment selects its task.
struct TodayBlock: View {
    @Environment(DailyTotalsStore.self) private var dailyTotals
    @Environment(TaskCatalogStore.self) private var catalog
    @State private var hovered: String?

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
        let focused = slices.first { $0.id == hovered } ?? slices.first { $0.id == selected }
        VStack(alignment: .leading, spacing: 8) {
            heading
            if !slices.isEmpty {
                DaySplitBar(slices: slices, selected: selected, hovered: $hovered)
                readout(of: focused, in: slices)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .help(dailyTotals.totalsExplanation)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("today-summary")
    }

    private var heading: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(.secondary)
            total
        }
        .fixedSize()
    }

    @ViewBuilder private var total: some View {
        if let seconds = dailyTotals.dayTotal, seconds >= 1 {
            DurationFigure(text: readableDuration(seconds), size: 30, weight: .medium)
                .accessibilityIdentifier("today-summary.total")
        } else {
            Text(dailyTotals.dayTotal == nil ? "Unavailable" : "Nothing tracked yet")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
                .accessibilityIdentifier("today-summary.total")
        }
    }

    private func sharePercent(_ share: Double) -> String { "\(Int((share * 100).rounded()))%" }

    /// The share of the day, so the readout adds the one number that the sidebar rows don't show.
    @ViewBuilder private func readout(of slice: DaySlice?, in slices: [DaySlice]) -> some View {
        if let slice {
            let share = slice.seconds / slices.reduce(0) { $0 + $1.seconds }
            HStack(spacing: 6) {
                Circle().fill(slice.color).frame(width: 7, height: 7)
                Text(slice.task.name)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: 4)
                Text(sharePercent(share))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
            .font(.caption)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(
                "\(slice.task.name), \(sharePercent(share)) of today")
        } else {
            Text("Nothing on this task today")
                .font(.caption)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
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

/// One segment per task with time today, as wide as the task's share. While a task is selected or hovered, the other
/// segments fade, so the focus is a tint and never an outline or a change of size.
private struct DaySplitBar: View {
    let slices: [DaySlice]
    let selected: String?
    @Binding var hovered: String?
    @Environment(TaskCatalogStore.self) private var catalog

    private let gap: CGFloat = 2
    private let minimumWidth: CGFloat = 5
    private let height: CGFloat = 10

    var body: some View {
        let focus = hovered ?? selected
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
                            .opacity(focus == nil || focus == slice.id ? 1 : 0.45)
                            .frame(width: max(minimumWidth, usable * slice.seconds / total), height: height)
                            .frame(maxHeight: .infinity)
                            .contentShape(Rectangle())
                            .animation(.easeOut(duration: 0.12), value: focus)
                    }
                    .buttonStyle(.plain)
                    .onHover { inside in
                        if inside {
                            hovered = slice.id
                        } else if hovered == slice.id {
                            hovered = nil
                        }
                    }
                    .help("\(slice.task.name), \(readableDuration(slice.seconds))")
                    .accessibilityLabel("\(slice.task.name), \(readableDuration(slice.seconds)) today")
                    .accessibilityAddTraits(selected == slice.id ? .isSelected : [])
                }
            }
        }
        .frame(height: 18)
        .accessibilityIdentifier("today-summary.bar")
    }
}
