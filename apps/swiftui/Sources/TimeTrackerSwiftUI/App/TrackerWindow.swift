import SwiftUI

@MainActor
struct TrackerWindow: View {
    let runtime: TrackerAppRuntime
    @ObservedObject private var store: TrackerStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.appearsActive) private var appearsActive
    @Environment(\.colorScheme) private var colorScheme
    @State private var columnVisibility: NavigationSplitViewVisibility = .all
    @ObservedObject private var presentation: TrackerTaskPresentationCoordinator
    let windowID: UUID

    init(runtime: TrackerAppRuntime, windowID: UUID) {
        self.runtime = runtime
        self.windowID = windowID
        store = runtime.store
        presentation = runtime.presentation
    }

    @ViewBuilder var body: some View {
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("-tt-ui-testing"),
           let suite = ProcessInfo.processInfo.environment["TT_UI_TEST_DEFAULTS_SUITE"],
           suite.hasPrefix("TimeTrackerUITests."),
           UUID(uuidString: String(suite.dropFirst("TimeTrackerUITests.".count))) != nil {
            content.accessibilityValue(colorScheme == .dark ? "Dark" : "Light")
        } else {
            content
        }
        #else
        content
        #endif
    }

    private var content: some View {
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
        .accessibilityIdentifier("tracker.window.content")
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
    @ObservedObject private var bulkArchiving: BulkTaskArchivingStore
    @Environment(\.openWindow) private var openWindow

    init(store: TrackerStore, preferWindow: @escaping () -> Void) {
        self.preferWindow = preferWindow
        creation = store.creation
        rename = store.rename
        bulkArchiving = store.bulkArchiving
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

        Button {
            guard bulkArchiving.canOpen else { return }
            preferWindow()
            bulkArchiving.open()
        } label: {
            Label("Archive inactive tasks", systemImage: "archivebox")
        }
        .disabled(!bulkArchiving.canOpen)
        .help("Archive inactive tasks")
        .accessibilityIdentifier("bulk-archive.open")
    }
}

@MainActor
private struct TrackerTaskToolbar: View {
    let preferWindow: () -> Void
    @ObservedObject var store: TrackerStore
    @ObservedObject private var activity: TrackerActivityStore
    @ObservedObject private var creation: TaskCreationStore
    @ObservedObject private var rename: TaskRenameStore
    @ObservedObject private var archiving: TaskArchivingStore

    init(store: TrackerStore, preferWindow: @escaping () -> Void) {
        self.preferWindow = preferWindow
        self.store = store
        activity = store.activity
        creation = store.creation
        rename = store.rename
        archiving = store.archiving
    }

    private var hasTaskEditor: Bool {
        creation.state.isPresented || rename.state.isPresented || archiving.state.isPresented
    }

    var body: some View {
        PendingTaskArchivingButton(archiving: archiving)
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

            let canChangeArchive = task.archived ? archiving.canUnarchive(taskID: task.id)
                : archiving.canArchive(taskID: task.id)
            Button {
                guard canChangeArchive else { return }
                preferWindow()
                if task.archived { archiving.unarchive(taskID: task.id) }
                else { archiving.openArchive(taskID: task.id) }
            } label: {
                Label(task.archived ? "Unarchive task" : "Archive task",
                      systemImage: task.archived ? "archivebox.fill" : "archivebox")
            }
            .disabled(!canChangeArchive)
            .help(!task.archived && store.active?.taskId == task.id
                  ? "Stop tracking before archiving this task"
                  : task.archived ? "Unarchive this task" : "Archive this task")

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
