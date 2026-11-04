import SwiftUI
import TrackerClient

struct TrackerElapsedText: View {
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        Text(tracking.timerText)
            .accessibilityIdentifier("tracking.elapsed")
    }
}
