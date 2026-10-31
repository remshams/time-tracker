import SwiftUI

@main
struct TimeTrackerApp: App {
    @NSApplicationDelegateAdaptor(TrackerApplicationDelegate.self) private var delegate

    var body: some Scene {
        // A Window scene exists at most once, so opening it again focuses the existing window.
        Window("Time Tracker", id: TrackerSceneID.tracker) {
            TrackerWindow(runtime: delegate.runtime)
        }
        .defaultSize(width: 1100, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
        .commands { TrackerApplicationMenus(runtime: delegate.runtime) }

        Window("Settings", id: TrackerSceneID.settings) {
            SettingsView(store: delegate.runtime.store)
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
