import AppKit
import SwiftUI
import TrackerClient

struct TrackerMenu: View {
    @ObservedObject var store: TrackerStore
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(store.connectionStatusText)
        if store.isStale { Text("Showing last confirmed state") }
        Text(store.runningTaskName)
        if let status = store.autoPauseStatusText { Text(status) }
        if let error = store.trackingError { Text(error) }
        if let elapsed = store.elapsed {
            Text(clockDuration(elapsed))
        }
        Divider()
        Button("Open Time Tracker") { openWindow(id: "tracker") }
        Button("Quit Time Tracker") { NSApplication.shared.terminate(nil) }
    }
}
