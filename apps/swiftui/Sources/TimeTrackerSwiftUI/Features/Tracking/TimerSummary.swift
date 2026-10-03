import AppKit
import SwiftUI
import TrackerClient

struct TimerSummary: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: store.active == nil ? "clock" : "timer")
                .font(.title2)
                .foregroundStyle(store.active == nil ? Color.secondary : Color.accentColor)

            VStack(alignment: .leading, spacing: 4) {
                Text(store.active == nil ? "Timer" : "Currently tracking")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(store.runningTaskName)
                    .font(.headline)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .help(store.runningTaskName)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            Text(store.timerDisplayText)
                .font(.system(.title2, design: .monospaced).weight(.medium))
                .monospacedDigit()
                .fixedSize()
        }
        .foregroundStyle(.primary)
        .padding(.horizontal, 24)
        .padding(.vertical, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: .controlBackgroundColor))
    }
}
