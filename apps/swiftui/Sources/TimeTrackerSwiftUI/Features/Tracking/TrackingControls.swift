import SwiftUI

struct TrackerElapsedText: View {
    let store: TrackerStore

    var body: some View {
        Text(store.timerText)
            .accessibilityIdentifier("tracking.elapsed")
    }
}
