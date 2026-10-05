import SwiftUI

@MainActor
struct TrackerWindow: View {
    let runtime: TrackerAppRuntime
    @ObservedObject private var store: TrackerStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.appearsActive) private var appearsActive
    @State private var columnVisibility: NavigationSplitViewVisibility = .all
    @ObservedObject private var presentation: TrackerTaskPresentationCoordinator
    let windowID: UUID

    init(runtime: TrackerAppRuntime, windowID: UUID) {
        self.runtime = runtime
        self.windowID = windowID
        store = runtime.store
        presentation = runtime.presentation
    }

    var body: some View {
        TrackerSplitLayout(columnVisibility: $columnVisibility) {
            TrackerSidebar(store: store)
                .toolbar {
                    ToolbarItemGroup(placement: .automatic) {
                        TrackerSidebarToolbar(store: store, preferWindow: { runtime.preferWindow(windowID) })
                    }
                }
        } detail: {
            TrackerDetailPresentation(store: store, presentation: runtime.presentation, windowID: windowID)
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        TrackerTaskToolbar(store: store, preferWindow: { runtime.preferWindow(windowID) })
                    }
                }
        }
        .navigationTitle(store.selectedTask?.name ?? "Time Tracker")
        .frame(minWidth: 760, minHeight: 480)
        .windowDismissBehavior(presentation.canClose(windowID) ? .enabled : .disabled)
        .onAppear {
            let action = openWindow
            runtime.installSceneOpener { action(id: TrackerSceneID.tracker, value: $0) }
            runtime.windowAppeared(windowID, phase: scenePhase)
            if appearsActive { runtime.preferWindow(windowID) }
        }
        .onChange(of: scenePhase) { _, phase in
            runtime.windowPhaseChanged(windowID, phase: phase)
        }
        .onChange(of: appearsActive) { _, isFocused in
            if isFocused { runtime.preferWindow(windowID) }
        }
        .onDisappear { runtime.windowDisappeared(windowID) }
    }
}

@MainActor
private struct TrackerSidebarToolbar: View {
    let preferWindow: () -> Void
    @ObservedObject private var creation: TaskCreationStore
    @ObservedObject private var rename: TaskRenameStore
    @Environment(\.openWindow) private var openWindow

    init(store: TrackerStore, preferWindow: @escaping () -> Void) {
        self.preferWindow = preferWindow
        creation = store.creation
        rename = store.rename
    }

    private var canCreate: Bool {
        creation.canOpen && !creation.state.isPresented && !rename.state.isPresented
    }

    var body: some View {
        Button {
            openWindow(id: TrackerSceneID.settings)
        } label: {
            Label("Settings", systemImage: "gearshape")
        }
        .help("Settings")

        Button {
            guard canCreate else { return }
            preferWindow()
            creation.open()
        } label: {
            Label("New task", systemImage: "plus")
        }
        .disabled(!canCreate)
        .help("New task")
    }
}

@MainActor
private struct TrackerTaskToolbar: View {
    let preferWindow: () -> Void
    @ObservedObject var store: TrackerStore
    @ObservedObject private var activity: TrackerActivityStore
    @ObservedObject private var creation: TaskCreationStore
    @ObservedObject private var rename: TaskRenameStore

    init(store: TrackerStore, preferWindow: @escaping () -> Void) {
        self.preferWindow = preferWindow
        self.store = store
        activity = store.activity
        creation = store.creation
        rename = store.rename
    }

    private var hasTaskEditor: Bool { creation.state.isPresented || rename.state.isPresented }

    var body: some View {
        if let task = store.selectedTask {
            Button {
                guard rename.canOpen, !hasTaskEditor else { return }
                preferWindow()
                rename.open(taskID: task.id)
            } label: {
                Label("Edit task name", systemImage: "pencil")
            }
            .disabled(!rename.canOpen || hasTaskEditor)
            .help("Edit task name")

            if !task.archived {
                let isRunning = store.active?.taskId == task.id
                let canTrack = isRunning ? activity.canStopTracking
                    : activity.canStartTracking(taskID: task.id)
                Button {
                    guard canTrack else { return }
                    if let active = store.active, active.taskId == task.id {
                        store.stopTracking(worklogID: active.id)
                    } else {
                        store.startTracking(taskID: task.id)
                    }
                } label: {
                    Label(isRunning ? "Stop tracking" : "Start tracking",
                          systemImage: isRunning ? "stop.fill" : "play.fill")
                }
                .disabled(!canTrack)
                .help(isRunning ? "Stop tracking this task" : store.active == nil
                      ? "Start tracking this task" : "Stop the current timer and start tracking this task")
            }
        }
    }
}
