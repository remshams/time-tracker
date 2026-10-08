import SwiftUI
#if DEBUG
import AppKit
#endif

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
            content.background {
                TrackerWindowAppearanceProbe(appearance: colorScheme == .dark ? "Dark" : "Light")
                    .frame(width: 1, height: 1)
            }
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

#if DEBUG
@MainActor
private struct TrackerWindowAppearanceProbe: NSViewRepresentable {
    let appearance: String

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField(labelWithString: appearance)
        field.textColor = .clear
        field.setAccessibilityIdentifier("tracker.window.appearance")
        field.setAccessibilityLabel("Window appearance")
        context.coordinator.observeResize(for: field)
        return field
    }

    func updateNSView(_ field: NSTextField, context: Context) {
        field.stringValue = appearance
        field.setAccessibilityValue(appearance)
    }

    static func dismantleNSView(_ field: NSTextField, coordinator: Coordinator) {
        coordinator.stop()
    }

    @MainActor
    final class Coordinator {
        private weak var field: NSTextField?
        private var resizeObserver: NSObjectProtocol?

        deinit {
            if let resizeObserver {
                DistributedNotificationCenter.default().removeObserver(resizeObserver)
            }
        }

        func observeResize(for field: NSTextField) {
            guard TrackerLaunchConfiguration.current.localDatabasePath != nil,
                  let suite = ProcessInfo.processInfo.environment["TT_UI_TEST_DEFAULTS_SUITE"] else { return }
            self.field = field
            resizeObserver = DistributedNotificationCenter.default().addObserver(
                forName: Notification.Name("\(suite).resize"), object: nil, queue: .main
            ) { [weak self] notification in
                MainActor.assumeIsolated { self?.resize(notification) }
            }
        }

        func stop() {
            if let resizeObserver {
                DistributedNotificationCenter.default().removeObserver(resizeObserver)
            }
            resizeObserver = nil
            field = nil
        }

        private func resize(_ notification: Notification) {
            guard let window = field?.window, window.isKeyWindow,
                  let screen = window.screen,
                  let width = notification.userInfo?["width"] as? NSNumber,
                  let height = notification.userInfo?["height"] as? NSNumber,
                  width.doubleValue.isFinite, height.doubleValue.isFinite,
                  width.doubleValue > 0, height.doubleValue > 0 else { return }
            let available = window.contentRect(forFrameRect: screen.visibleFrame).size
            let minimum = window.contentMinSize
            guard minimum.width <= available.width, minimum.height <= available.height else { return }
            window.setContentSize(NSSize(
                width: max(minimum.width, min(CGFloat(width.doubleValue), available.width)),
                height: max(minimum.height, min(CGFloat(height.doubleValue), available.height))
            ))
        }
    }
}
#endif

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
