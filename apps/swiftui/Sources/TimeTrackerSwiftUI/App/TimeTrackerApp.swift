import SwiftUI
import TrackerClient

@main
struct TimeTrackerApp: App {
    @NSApplicationDelegateAdaptor(TrackerApplicationDelegate.self) private var delegate

    var body: some Scene {
        // A Window scene exists at most once, so opening it again focuses the existing window.
        Window("Time Tracker", id: TrackerSceneID.tracker) {
            TrackerWindow(runtime: delegate.runtime)
                .environment(delegate.runtime.features.trackingActions)
                .environment(delegate.runtime.features.taskCatalog)
                .environment(delegate.runtime.features.worklogHistory)
                .environment(delegate.runtime.features.connection)
                .environment(delegate.runtime.features.tracking)
                .environment(delegate.runtime.features.dailyTotals)
                .environment(delegate.runtime.features.trackingAutomation)
                .environment(delegate.runtime.features.taskRename)
                .environment(delegate.runtime.features.taskArchiving)
                .environment(delegate.runtime.features.taskCreation)
                .environment(delegate.runtime.features.worklogDeletion)
                .environment(delegate.runtime.features.worklogMove)
                .environment(delegate.runtime.features.bulkTaskArchiving)
                .environment(delegate.runtime.features.worklogCorrection)
        }
        .defaultSize(width: 1100, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
        .commands { TrackerApplicationMenus(runtime: delegate.runtime) }

        Window("Settings", id: TrackerSceneID.settings) {
            SettingsView(connection: delegate.runtime.features.connection, client: delegate.runtime.client)
            .environment(delegate.runtime.features.connection)
            .environment(delegate.runtime.features.trackingAutomation)
            .environment(delegate.runtime.menuBarPreferences)
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
