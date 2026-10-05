import SwiftUI

@MainActor
struct TrackerApplicationMenus: Commands {
    let runtime: TrackerAppRuntime
    @ObservedObject private var creation: TaskCreationStore
    @ObservedObject private var rename: TaskRenameStore
    @Environment(\.openWindow) private var openWindow

    init(runtime: TrackerAppRuntime) {
        self.runtime = runtime
        creation = runtime.store.creation
        rename = runtime.store.rename
    }

    private var canCreateTask: Bool {
        creation.canOpen && !creation.state.isPresented && !rename.state.isPresented
    }

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

            Button("New Window") {
                installSceneOpener()
                openWindow(id: TrackerSceneID.tracker)
            }
            .keyboardShortcut("n", modifiers: [.command, .shift])
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
