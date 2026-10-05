import AppKit

@MainActor
final class TrackerApplicationDelegate: NSObject, NSApplicationDelegate {
    let runtime = TrackerAppRuntime()

    func applicationDidFinishLaunching(_ notification: Notification) {
        runtime.start()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        runtime.showTracker()
        return false
    }

    func applicationWillTerminate(_ notification: Notification) {
        runtime.stop()
    }
}

@MainActor
final class TrackerAppRuntime {
    let store = TrackerStore()
    private lazy var statusItem = TrackerStatusItemController(store: store)
    lazy var presentation = TrackerTaskPresentationCoordinator(
        store: store,
        presentingWindow: { [weak self] needsEditor in
            guard let self else { return nil }
            if needsEditor { return self.showTracker() }
            guard !NSApplication.shared.isHidden else { return nil }
            return self.preferredWindow(visibleOnly: true)
        }
    )
    private var windows: [TrackerWindowReference] = []
    private var openTracker: (() -> Void)?
    private var openingTracker = false
    private var revealWhenAvailable = false
    private var isRunning = false

    func start() {
        guard !isRunning else { return }
        isRunning = true
        statusItem.start { [weak self] in self?.showTracker() }
        presentation.start()
    }

    func stop() {
        presentation.stop()
        openTracker = nil
    }

    func installSceneOpener(_ openTracker: @escaping () -> Void) {
        self.openTracker = openTracker
        start()
        if revealWhenAvailable { showTracker() }
    }

    func register(_ window: NSWindow) {
        windows.removeAll { $0.window == nil || $0.window === window }
        windows.append(TrackerWindowReference(window))
        openingTracker = false
        if revealWhenAvailable {
            revealWhenAvailable = false
            reveal(window)
        }
        presentation.windowAvailable()
    }

    func unregister(_ window: NSWindow) {
        windows.removeAll { $0.window == nil || $0.window === window }
        presentation.windowClosed(window)
    }

    @discardableResult
    func showTracker() -> NSWindow? {
        if let window = preferredWindow(visibleOnly: false) {
            revealWhenAvailable = false
            reveal(window)
            presentation.windowAvailable()
            return window
        }
        revealWhenAvailable = true
        if !openingTracker, let openTracker {
            openingTracker = true
            openTracker()
        }
        return nil
    }

    private func preferredWindow(visibleOnly: Bool) -> NSWindow? {
        let candidates = windows.compactMap(\.window).filter {
            !visibleOnly || ($0.isVisible && !$0.isMiniaturized)
        }
        let app = NSApplication.shared
        return candidates.first(where: { $0 === app.keyWindow })
            ?? candidates.first(where: { $0 === app.mainWindow })
            ?? candidates.last
    }

    private func reveal(_ window: NSWindow) {
        if window.isMiniaturized { window.deminiaturize(nil) }
        window.makeKeyAndOrderFront(nil)
        if #available(macOS 14, *) {
            NSApplication.shared.activate()
        } else {
            NSApplication.shared.activate(ignoringOtherApps: true)
        }
    }
}

@MainActor
private final class TrackerWindowReference {
    weak var window: NSWindow?
    init(_ window: NSWindow) { self.window = window }
}
