import AppKit
import SwiftUI
import TrackerClient

struct WorklogRow: View {
    let worklog: WorklogItem
    let active: WorklogItem?
    let elapsed: TimeInterval?

    private var duration: TimeInterval {
        if active?.id == worklog.id { return elapsed ?? 0 }
        guard let start = timestamp(worklog.start), let end = timestamp(worklog.end) else { return 0 }
        return max(0, end.timeIntervalSince(start))
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(localDay(worklog.start))
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.primary)
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 12)
            Text(clockDuration(duration))
                .font(.system(.title3, design: .monospaced).weight(.medium))
                .foregroundStyle(.primary)
                .monospacedDigit()
                .fixedSize()
        }
        .padding(16)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        }
    }
}
