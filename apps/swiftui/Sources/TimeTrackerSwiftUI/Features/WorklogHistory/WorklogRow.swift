import SwiftUI
import TrackerClient

struct WorklogRow: View {
    let worklog: WorklogItem
    let active: WorklogItem?
    let timer: TrackerTimerStore

    private var duration: TimeInterval {
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
            Group {
                if active?.id == worklog.id {
                    TrackerElapsedText(timer: timer)
                } else {
                    Text(clockDuration(duration))
                }
            }
                .font(.system(.title3, design: .monospaced).weight(.medium))
                .foregroundStyle(.primary)
                .monospacedDigit()
                .fixedSize()
        }
        .padding(16)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(.quaternary, lineWidth: 1)
        }
    }
}
