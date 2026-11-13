import SwiftUI
import TrackerClient

struct TrackerElapsedText: View {
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        Text(readableDuration(clockText: tracking.timerText))
            .accessibilityLabel(tracking.timerText)
            .accessibilityIdentifier("tracking.elapsed")
    }
}
