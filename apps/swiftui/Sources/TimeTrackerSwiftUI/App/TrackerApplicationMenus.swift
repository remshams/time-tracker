import SwiftUI

struct TrackerApplicationMenus: Commands {
    let runtime: TrackerAppRuntime
    private let creation: TaskCreationStore
    @Environment(\.openWindow) private var openWindow

    init(runtime: TrackerAppRuntime) {
        self.runtime = runtime
        creation = runtime.store.creation
    }

    private var canCreateTask: Bool { creation.canOpen && !runtime.presentation.hasPresentation }

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Task") {
                guard canCreateTask else { return }
                installSceneOpener()
                runtime.showTracker()
                creation.open()
            }
            .keyboardShortcut("n")
            .disabled(!canCreateTask)
        }

        CommandGroup(replacing: .appSettings) {
            Button("Settings...") { openWindow(id: TrackerSceneID.settings) }
                .keyboardShortcut(",")
        }

        CommandGroup(after: .windowArrangement) {
            Button("Show Time Tracker") {
                installSceneOpener()
                runtime.showTracker()
            }
            .keyboardShortcut("0")
        }
    }

    private func installSceneOpener() {
        let action = openWindow
        runtime.installSceneOpener { action(id: TrackerSceneID.tracker) }
    }
}
