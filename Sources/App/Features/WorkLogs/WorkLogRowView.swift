import SwiftUI

struct WorkLogRowView: View {
    private static let timeColumnWidth: CGFloat = 130
    private static let durationColumnWidth: CGFloat = 80

    let entry: WorkLogEntry

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: AppSpacing.compact) {
            Text(timeRangeText)
                .font(.body.monospacedDigit())
                .frame(width: Self.timeColumnWidth, alignment: .leading)

            Text(durationText)
                .font(.body.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(width: Self.durationColumnWidth, alignment: .leading)

            if let description = entry.description {
                Text(description)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }

            Spacer(minLength: 0)
        }
        .padding(.vertical, AppSpacing.tight)
    }

    private var timeRangeText: String {
        let start = entry.startedAt.formatted(date: .omitted, time: .shortened)
        if let endedAt = entry.endedAt {
            let end = endedAt.formatted(date: .omitted, time: .shortened)
            return "\(start) – \(end)"
        }
        let running = String(localized: "work-log-row.running", defaultValue: "Running")
        return "\(start) – \(running)"
    }

    private var durationText: String {
        guard let duration = entry.duration else {
            return String(localized: "work-log-row.duration.unknown", defaultValue: "–")
        }
        let totalSeconds = Int(duration.components.seconds)
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        return "\(minutes)m"
    }
}
