import SwiftUI

@main
@MainActor
struct TimeTrackerApp: App {
    @NSApplicationDelegateAdaptor(TrackerApplicationDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup("Time Tracker", id: TrackerSceneID.tracker) {
            TrackerWindow(runtime: delegate.runtime)
        }
        .defaultSize(width: 1100, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
        .commands { TrackerApplicationMenus(runtime: delegate.runtime) }

        // Window gives the gear and menu command a public opening API on macOS 13.
        Window("Settings", id: TrackerSceneID.settings) {
            ConnectionSettingsView(store: delegate.runtime.store)
                .frame(height: 680)
        }
        .defaultSize(width: 540, height: 680)
        .windowResizability(.contentSize)
        .commandsRemoved()
    }
}

enum TrackerSceneID {
    static let tracker = "tracker"
    static let settings = "settings"
}
