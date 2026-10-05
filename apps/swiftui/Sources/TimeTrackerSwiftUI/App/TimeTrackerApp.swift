import AppKit
import SwiftUI
import TrackerClient

@main
@MainActor
struct TimeTrackerApp: App {
    @StateObject private var model = TrackerAppModel()

    var body: some Scene {
        let store = model.store
        WindowGroup("Time Tracker", id: "tracker") {
            TrackerWindow(store: store, statusItem: model.statusItem)
        }
        .defaultSize(width: 1100, height: 760)
        .commands { TaskCreationCommands() }

        Settings {
            ConnectionSettingsView(store: store)
        }
    }
}

@MainActor
private final class TrackerAppModel: ObservableObject {
    // Views observe their own adapters. Session updates must not rebuild the scenes.
    let store: TrackerStore
    let statusItem: TrackerStatusItemController

    init() {
        store = TrackerStore()
        statusItem = TrackerStatusItemController(store: store)
    }
}
