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

        MenuBarExtra(isInserted: $menuBarInserted) {
            TrackerMenu(store: store)
        } label: {
            Image(systemName: menuBarSymbol)
                .accessibilityLabel(Text(menuBarStatus))
                .help(menuBarStatus)
        }

        Settings {
            ConnectionSettingsView(store: store)
        }
    }

    private var menuBarSymbol: String {
        if store.isStale { return "questionmark.circle" }
        return store.active == nil ? "clock" : "play.circle.fill"
    }

    private var menuBarStatus: String {
        if store.isStale { return "Time Tracker: Tracking status unavailable" }
        if store.active != nil { return "Time Tracker: Tracking \(store.runningTaskName)" }
        return "Time Tracker: \(store.autoPauseStatusText ?? "No timer running")"
    }
}
