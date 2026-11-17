import SwiftUI
import TrackerClient

/// The top of the sidebar: today's total across all tasks, one bar that splits it by task, and a readout that names
/// the hovered or selected task and its share. Clicking a segment shows its task, on the other tab if needed.
struct TodaySummary: View {
    @Environment(DailyTotalsStore.self) private var dailyTotals
    @Environment(TaskCatalogStore.self) private var catalog
    @State private var hovered: String?

    var body: some View {
        let shares = dailyTotals.shares
        let selected = catalog.selectedTaskID
        // The bar and the readout keep their place without shares, so the list doesn't move when the first time
        // arrives.
        VStack(alignment: .leading, spacing: Spacing.small) {
            heading
            DailyTotalsBar(shares: shares, selected: selected, hovered: $hovered)
            readout(shares.first { $0.id == hovered } ?? shares.first { $0.id == selected })
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .help(dailyTotals.totalsExplanation)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("today-summary")
    }

    private var heading: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.small) {
            Text(dailyTotals.status == .cached ? "Today, cached" : "Today")
                .font(TextRole.heading)
                .foregroundStyle(.secondary)
            total
                .accessibilityIdentifier("today-summary.total")
        }
    }

    @ViewBuilder private var total: some View {
        if let seconds = dailyTotals.dayTotal.map(Int.init), seconds > 0 {
            DurationLabel(duration: DurationText(seconds: seconds))
                .font(TextRole.dayTotal)
                .foregroundStyle(.primary)
                .lineLimit(1)
                .accessibilityLabel(spokenDuration(seconds))
        } else {
            Text(placeholder)
                .font(TextRole.detail)
                .foregroundStyle(.tertiary)
        }
    }

    private var placeholder: String {
        if dailyTotals.dayTotal != nil { return "Nothing tracked yet" }
        return dailyTotals.status == .loading ? "Loading" : "Unavailable"
    }

    /// The readout keeps its line while no share is focused, so the list below doesn't move. VoiceOver reads the same
    /// facts from the segments.
    private func readout(_ share: DailyTotalShare?) -> some View {
        HStack(spacing: Spacing.xSmall) {
            Circle()
                .fill(share.map { ColorRole.task(TaskColor.forTaskID($0.taskID)) } ?? .clear)
                .frame(width: IconSize.marker, height: IconSize.marker)
            Text(share?.name ?? " ")
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: Spacing.xSmall)
            Text(share.map { "\($0.percent)%" } ?? "")
                .monospacedDigit()
                .foregroundStyle(.secondary)
        }
        .font(TextRole.caption)
        .opacity(share == nil ? 0 : 1)
        .accessibilityHidden(true)
    }
}

/// One segment per task with time today, as wide as the task's share, or an empty track without shares. Every segment
/// keeps its full color; the selected one is taller.
private struct DailyTotalsBar: View {
    let shares: [DailyTotalShare]
    let selected: String?
    @Binding var hovered: String?
    @Environment(TaskCatalogStore.self) private var catalog

    private static let height: CGFloat = 8
    /// The selected task's segment. The bar keeps this height, so selecting a task doesn't move the list.
    private static let selectedHeight: CGFloat = 12
    /// The narrowest segment, so a task with a few seconds stays visible and clickable.
    private static let minimumSegment: CGFloat = 4

    var body: some View {
        GeometryReader { proxy in
            if shares.isEmpty {
                RoundedRectangle(cornerRadius: CornerRadius.small, style: .continuous)
                    .fill(.quaternary)
                    .frame(height: Self.height)
                    .frame(maxHeight: .infinity)
            } else {
                let layout = DailyTotalsBarLayout(
                    fractions: shares.map(\.fraction), width: proxy.size.width, gap: Spacing.xxSmall,
                    minimum: Self.minimumSegment)
                HStack(spacing: layout.gap) {
                    ForEach(Array(zip(shares, layout.widths)), id: \.0.id) { share, width in
                        segment(share, width: width)
                    }
                }
            }
        }
        .frame(height: Self.selectedHeight)
    }

    private func segment(_ share: DailyTotalShare, width: CGFloat) -> some View {
        let color = ColorRole.task(TaskColor.forTaskID(share.taskID))
        let isSelected = share.id == selected
        return Button {
            catalog.showTask(id: share.taskID)
        } label: {
            RoundedRectangle(cornerRadius: CornerRadius.small, style: .continuous)
                .fill(color)
                .frame(width: width, height: isSelected ? Self.selectedHeight : Self.height)
                .frame(height: Self.selectedHeight)
                .contentShape(Rectangle())
                .animation(Motion.standard, value: isSelected)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            if inside {
                hovered = share.id
            } else if hovered == share.id {
                hovered = nil
            }
        }
        .help("\(share.name), \(share.percent)%")
        .accessibilityLabel(share.accessibilityLabel)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .accessibilityIdentifier("today-summary.share.\(share.taskID)")
    }
}
