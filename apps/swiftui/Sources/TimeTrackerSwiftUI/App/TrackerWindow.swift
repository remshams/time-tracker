import SwiftUI
#if DEBUG
    import AppKit
#endif

struct TrackerWindow: View {
    let runtime: TrackerAppRuntime
    private let store: TrackerStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.colorScheme) private var colorScheme
    @State private var columnVisibility: NavigationSplitViewVisibility = .all
    private let presentation: TrackerTaskPresentationCoordinator

    init(runtime: TrackerAppRuntime) {
        self.runtime = runtime
        store = runtime.store
        presentation = runtime.presentation
    }

    @ViewBuilder var body: some View {
        #if DEBUG
            if TrackerLaunchConfiguration.uiTestSuite != nil {
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
                        TrackerSidebarToolbar(store: store)
                    }
                }
        } detail: {
            TrackerDetailPresentation(store: store)
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        TrackerTaskToolbar(store: store)
                    }
                }
        }
        .environment(presentation)
        .navigationTitle(store.selectedTask?.name ?? "Time Tracker")
        .frame(minWidth: 760, minHeight: 480)
        .windowDismissBehavior(presentation.canCloseWindow ? .enabled : .disabled)
        .onAppear { runtime.installSceneOpener(openWindow) }
    }
}

#if DEBUG
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

        final class Coordinator {
            private weak var field: NSTextField?
            private var resizeObserver: NSObjectProtocol?

            isolated deinit {
                if let resizeObserver {
                    DistributedNotificationCenter.default().removeObserver(resizeObserver)
                }
            }

            func observeResize(for field: NSTextField) {
                guard let suite = TrackerLaunchConfiguration.uiTestSuite else { return }
                self.field = field
                resizeObserver = DistributedNotificationCenter.default().addObserver(
                    forName: Notification.Name("\(suite).resize"), object: nil, queue: .main
                ) { [weak self] notification in
                    // Notification isn't Sendable, so read the requested size before entering the main actor.
                    let width = (notification.userInfo?["width"] as? NSNumber)?.doubleValue
                    let height = (notification.userInfo?["height"] as? NSNumber)?.doubleValue
                    MainActor.assumeIsolated { self?.resize(width: width, height: height) }
                }
            }

            func stop() {
                if let resizeObserver {
                    DistributedNotificationCenter.default().removeObserver(resizeObserver)
                }
                resizeObserver = nil
                field = nil
            }

            private func resize(width: Double?, height: Double?) {
                guard let window = field?.window, window.isKeyWindow,
                    let screen = window.screen,
                    let width, let height,
                    width.isFinite, height.isFinite,
                    width > 0, height > 0
                else { return }
                let available = window.contentRect(forFrameRect: screen.visibleFrame).size
                let minimum = window.contentMinSize
                guard minimum.width <= available.width, minimum.height <= available.height else { return }
                window.setContentSize(
                    NSSize(
                        width: max(minimum.width, min(CGFloat(width), available.width)),
                        height: max(minimum.height, min(CGFloat(height), available.height))
                    ))
            }
        }
    }
#endif

private struct TrackerSidebarToolbar: View {
    private let creation: TaskCreationStore
    private let bulkArchiving: BulkTaskArchivingStore
    @Environment(\.openWindow) private var openWindow
    @Environment(TrackerTaskPresentationCoordinator.self) private var presentation

    init(store: TrackerStore) {
        creation = store.creation
        bulkArchiving = store.bulkArchiving
    }

    private var canCreate: Bool { creation.canOpen && !presentation.hasPresentation }
    private var canOpenBulkArchiving: Bool { bulkArchiving.canOpen && !presentation.hasPresentation }

    var body: some View {
        Button {
            openWindow(id: TrackerSceneID.settings)
        } label: {
            Label("Settings", systemImage: "gearshape")
        }
        .help("Settings")

        Button {
            guard canCreate else { return }
            creation.open()
        } label: {
            Label("New task", systemImage: "plus")
        }
        .disabled(!canCreate)
        .help("New task")

        Button {
            guard canOpenBulkArchiving else { return }
            bulkArchiving.open()
        } label: {
            Label("Archive inactive tasks", systemImage: "archivebox")
        }
        .disabled(!canOpenBulkArchiving)
        .help("Archive inactive tasks")
        .accessibilityIdentifier("bulk-archive.open")
    }
}

private struct TrackerTaskToolbar: View {
    let store: TrackerStore
    private let rename: TaskRenameStore
    private let archiving: TaskArchivingStore
    @Environment(TrackerTaskPresentationCoordinator.self) private var presentation

    init(store: TrackerStore) {
        self.store = store
        rename = store.rename
        archiving = store.archiving
    }

    var body: some View {
        if let task = store.selectedTask {
            let canRename = rename.canOpen && !presentation.hasPresentation
            Button {
                guard canRename else { return }
                rename.open(taskID: task.id)
            } label: {
                Label("Edit task name", systemImage: "pencil")
            }
            .disabled(!canRename)
            .help("Edit task name")

            let canChangeArchive =
                !presentation.hasPresentation
                && (task.archived
                    ? archiving.canUnarchive(taskID: task.id)
                    : archiving.canArchive(taskID: task.id))
            Button {
                guard canChangeArchive else { return }
                if task.archived {
                    archiving.unarchive(taskID: task.id)
                } else {
                    archiving.openArchive(taskID: task.id)
                }
            } label: {
                Label(
                    task.archived ? "Unarchive task" : "Archive task",
                    systemImage: task.archived ? "archivebox.fill" : "archivebox")
            }
            .disabled(!canChangeArchive)
            .help(
                !task.archived && store.active?.taskId == task.id
                    ? "Stop tracking before archiving this task"
                    : task.archived ? "Unarchive this task" : "Archive this task")

            if !task.archived {
                let isRunning = store.active?.taskId == task.id
                let canTrack =
                    isRunning
                    ? store.canStopTracking
                    : store.canStartTracking(taskID: task.id)
                Button {
                    guard canTrack else { return }
                    if let active = store.active, active.taskId == task.id {
                        store.stopTracking(worklogID: active.id)
                    } else {
                        store.startTracking(taskID: task.id)
                    }
                } label: {
                    Label(
                        isRunning ? "Stop tracking" : "Start tracking",
                        systemImage: isRunning ? "stop.fill" : "play.fill")
                }
                .disabled(!canTrack)
                .help(
                    isRunning
                        ? "Stop tracking this task"
                        : store.active == nil
                            ? "Start tracking this task" : "Stop the current timer and start tracking this task")
            }
        }
    }
}
