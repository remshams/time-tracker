import AppKit
import SwiftUI
import TrackerClient

@main
@MainActor
struct TimeTrackerApp: App {
    @StateObject private var store = TrackerStore()
    @State private var menuBarInserted = true

    var body: some Scene {
        WindowGroup("Time Tracker", id: "tracker") {
            TrackerWindow(store: store)
        }
        .defaultSize(width: 1100, height: 760)
        .windowToolbarStyle(.unifiedCompact)

        MenuBarExtra("Time Tracker", systemImage: "clock", isInserted: $menuBarInserted) {
            TrackerMenu(store: store)
        }

        Settings {
            ConnectionSettingsView(store: store)
        }
    }
}
