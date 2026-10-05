import SwiftUI

@main
@MainActor
struct TimeTrackerApp: App {
    @NSApplicationDelegateAdaptor(TrackerApplicationDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup("Time Tracker", id: TrackerSceneID.tracker, for: UUID.self) { windowID in
            let id: UUID? = windowID.wrappedValue
            if let id {
                TrackerWindow(runtime: delegate.runtime, windowID: id)
            }
        } defaultValue: {
            UUID()
        }
        .defaultSize(width: 1100, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
        .commands { TrackerApplicationMenus(runtime: delegate.runtime) }

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
