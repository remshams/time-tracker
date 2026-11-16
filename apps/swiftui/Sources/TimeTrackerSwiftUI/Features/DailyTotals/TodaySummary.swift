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
        VStack(alignment: .leading, spacing: Spacing.small) {
            heading
            if !shares.isEmpty {
                DailyTotalsBar(shares: shares, selected: selected, hovered: $hovered)
                readout(shares.first { $0.id == hovered } ?? shares.first { $0.id == selected })
            }
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
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("today-summary.total")
    }

    @ViewBuilder private var total: some View {
        if let dayTotal = dailyTotals.dayTotal, Int(dayTotal) > 0 {
            DurationLabel(duration: DurationText(seconds: Int(dayTotal)))
                .font(TextRole.dayTotal)
                .foregroundStyle(.primary)
                .lineLimit(1)
                .accessibilityLabel(spokenDuration(Int(dayTotal)))
        } else {
            Text(dailyTotals.dayTotal == nil ? "Unavailable" : "Nothing tracked yet")
                .font(TextRole.detail)
                .foregroundStyle(.tertiary)
        }
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

/// One segment per task with time today, as wide as the task's share. Every segment keeps its full color; a line
/// under the selected one marks it.
private struct DailyTotalsBar: View {
    let shares: [DailyTotalShare]
    let selected: String?
    @Binding var hovered: String?
    @Environment(TaskCatalogStore.self) private var catalog

    var body: some View {
        GeometryReader { proxy in
            let usable = max(0, proxy.size.width - Spacing.xxSmall * CGFloat(shares.count - 1))
            HStack(alignment: .top, spacing: Spacing.xxSmall) {
                ForEach(shares) { share in
                    segment(share, width: max(BarSize.minimumSegment, usable * share.fraction))
                }
            }
        }
        .frame(height: BarSize.height + Spacing.xxSmall + BarSize.underline)
    }

    private func segment(_ share: DailyTotalShare, width: CGFloat) -> some View {
        let color = ColorRole.task(TaskColor.forTaskID(share.taskID))
        let isSelected = share.id == selected
        return Button {
            catalog.showTask(id: share.taskID)
        } label: {
            VStack(spacing: Spacing.xxSmall) {
                RoundedRectangle(cornerRadius: CornerRadius.small, style: .continuous)
                    .fill(color)
                    .frame(height: BarSize.height)
                Capsule()
                    .fill(color)
                    .frame(height: BarSize.underline)
                    .opacity(isSelected ? 1 : 0)
            }
            .frame(width: width)
            .contentShape(Rectangle())
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
