import SwiftUI
import TrackerClient

@main
struct TimeTrackerApp: App {
    @NSApplicationDelegateAdaptor(TrackerApplicationDelegate.self) private var delegate

    var body: some Scene {
        // A Window scene exists at most once, so opening it again focuses the existing window.
        Window("Time Tracker", id: TrackerSceneID.tracker) {
            TrackerWindow(runtime: delegate.runtime)
                .environment(delegate.runtime.store.taskCatalog)
                .environment(delegate.runtime.store.worklogHistory)
                .environment(delegate.runtime.store.connection)
                .environment(delegate.runtime.store.tracking)
                .environment(delegate.runtime.store.dailyTotals)
                .environment(delegate.runtime.store.trackingAutomation)
                .environment(delegate.runtime.store.taskRename)
                .environment(delegate.runtime.store.taskArchiving)
                .environment(delegate.runtime.store.taskCreation)
                .environment(delegate.runtime.store.worklogDeletion)
                .environment(delegate.runtime.store.worklogMove)
                .environment(delegate.runtime.store.bulkTaskArchiving)
        }
        .defaultSize(width: 1100, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
        .commands { TrackerApplicationMenus(runtime: delegate.runtime) }

        Window("Settings", id: TrackerSceneID.settings) {
            SettingsView(store: delegate.runtime.store)
                .environment(delegate.runtime.store.connection)
                .environment(delegate.runtime.store.trackingAutomation)
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
