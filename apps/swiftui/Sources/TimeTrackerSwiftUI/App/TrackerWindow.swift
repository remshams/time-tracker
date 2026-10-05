import AppKit
import Combine
import SwiftUI

@MainActor
struct TrackerWindow: View {
    let runtime: TrackerAppRuntime
    @ObservedObject private var store: TrackerStore
    @Environment(\.openWindow) private var openWindow
    @State private var columnVisibility: NavigationSplitViewVisibility = .all
    @State private var windowID: ObjectIdentifier?

    init(runtime: TrackerAppRuntime) {
        self.runtime = runtime
        store = runtime.store
    }

    var body: some View {
        TrackerSplitLayout(columnVisibility: $columnVisibility) {
            TrackerSidebar(store: store)
                .toolbar {
                    ToolbarItemGroup(placement: .automatic) {
                        TrackerSidebarToolbar(store: store)
                    }
                }
        } detail: {
            TrackerDetailPresentation(store: store, presentation: runtime.presentation, windowID: windowID)
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        TrackerTaskToolbar(store: store)
                    }
                }
        }
        .navigationTitle(store.selectedTask?.name ?? "Time Tracker")
        .frame(minWidth: 760, minHeight: 480)
        .background {
            TrackerWindowRegistration(runtime: runtime) { windowID = $0 }
                .frame(width: 0, height: 0)
        }
        .onAppear {
            let action = openWindow
            runtime.installSceneOpener { action(id: TrackerSceneID.tracker) }
        }
    }
}

@MainActor
private struct TrackerSidebarToolbar: View {
    @ObservedObject private var creation: TaskCreationStore
    @ObservedObject private var rename: TaskRenameStore
    @Environment(\.openWindow) private var openWindow

    init(store: TrackerStore) {
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
    @ObservedObject var store: TrackerStore
    @ObservedObject private var activity: TrackerActivityStore
    @ObservedObject private var creation: TaskCreationStore
    @ObservedObject private var rename: TaskRenameStore

    init(store: TrackerStore) {
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

// SwiftUI creates the window. This adapter only identifies it and protects shared dialogs.
@MainActor
private struct TrackerWindowRegistration: NSViewRepresentable {
    let runtime: TrackerAppRuntime
    let onWindowChange: (ObjectIdentifier?) -> Void

    func makeNSView(context: Context) -> TrackerWindowRegistrationView {
        TrackerWindowRegistrationView(runtime: runtime, onWindowChange: onWindowChange)
    }

    func updateNSView(_ nsView: TrackerWindowRegistrationView, context: Context) {
        nsView.onWindowChange = onWindowChange
        nsView.installCloseGuard()
    }

    static func dismantleNSView(_ nsView: TrackerWindowRegistrationView, coordinator: ()) {
        nsView.detach()
    }
}

@MainActor
private final class TrackerWindowRegistrationView: NSView {
    var onWindowChange: (ObjectIdentifier?) -> Void
    private let runtime: TrackerAppRuntime
    private weak var registeredWindow: NSWindow?
    private let closeGuard: TrackerWindowDelegateProxy
    private var subscriptions = Set<AnyCancellable>()

    init(runtime: TrackerAppRuntime, onWindowChange: @escaping (ObjectIdentifier?) -> Void) {
        self.runtime = runtime
        self.onWindowChange = onWindowChange
        closeGuard = TrackerWindowDelegateProxy(presentation: runtime.presentation)
        super.init(frame: .zero)
        closeGuard.onClose = { [weak self] in self?.detach() }
    }

    required init?(coder: NSCoder) {
        fatalError("TrackerWindowRegistrationView requires a runtime")
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard registeredWindow !== window else { return }
        detach()
        guard let window else { return }
        registeredWindow = window
        installCloseGuard()
        runtime.register(window)
        publishWindowID(ObjectIdentifier(window))

        let center = NotificationCenter.default
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didDeminiaturizeNotification] {
            center.publisher(for: name, object: window)
                .sink { [weak self] _ in self?.runtime.presentation.windowAvailable() }
                .store(in: &subscriptions)
        }
        center.publisher(for: NSApplication.didUnhideNotification)
            .sink { [weak self] _ in self?.runtime.presentation.windowAvailable() }
            .store(in: &subscriptions)
    }

    func installCloseGuard() {
        guard let window = registeredWindow, window.delegate !== closeGuard else { return }
        // Forward the delegate installed by SwiftUI, including its scene cleanup callbacks.
        closeGuard.forwardedDelegate = window.delegate
        window.delegate = closeGuard
    }

    func detach() {
        subscriptions.removeAll()
        guard let window = registeredWindow else { return }
        registeredWindow = nil
        if window.delegate === closeGuard { window.delegate = closeGuard.forwardedDelegate }
        closeGuard.forwardedDelegate = nil
        runtime.unregister(window)
        publishWindowID(nil)
    }

    private func publishWindowID(_ id: ObjectIdentifier?) {
        // View attachment can happen during an update; publish after SwiftUI finishes it.
        DispatchQueue.main.async { [weak self] in
            guard let self, self.registeredWindow.map(ObjectIdentifier.init) == id else { return }
            self.onWindowChange(id)
        }
    }
}

@MainActor
private final class TrackerWindowDelegateProxy: NSObject, NSWindowDelegate {
    weak var forwardedDelegate: NSWindowDelegate?
    var onClose: (() -> Void)?
    private let presentation: TrackerTaskPresentationCoordinator

    init(presentation: TrackerTaskPresentationCoordinator) {
        self.presentation = presentation
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard sender.attachedSheet == nil, presentation.canClose(sender) else { return false }
        return forwardedDelegate?.windowShouldClose?(sender) ?? true
    }

    func windowWillClose(_ notification: Notification) {
        // Let SwiftUI remove its scene before unregistering the native identity.
        let delegate = forwardedDelegate
        delegate?.windowWillClose?(notification)
        onClose?()
    }

    nonisolated override func responds(to selector: Selector!) -> Bool {
        if super.responds(to: selector) { return true }
        return MainActor.assumeIsolated {
            forwardedDelegate?.responds(to: selector) == true
        }
    }

    nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
        let target = MainActor.assumeIsolated {
            TrackerForwardingTarget(value: forwardedDelegate?.responds(to: selector) == true
                                    ? forwardedDelegate : nil)
        }
        return target.value ?? super.forwardingTarget(for: selector)
    }
}

// Objective-C forwarding returns this object synchronously on the main actor.
private struct TrackerForwardingTarget: @unchecked Sendable {
    let value: Any?
}
