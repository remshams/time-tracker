import AppKit

@MainActor
final class TrackerWindowController: NSWindowController, NSWindowDelegate {
    let trackerWindow: NSWindow
    let splitController: TrackerSplitViewController
    var onClose: (() -> Void)?
    private let presentation: TrackerTaskPresentationCoordinator

    init(store: TrackerStore, presentation: TrackerTaskPresentationCoordinator,
         openSettings: @escaping () -> Void) {
        self.presentation = presentation
        // Create the window before its hosted views so presentation uses its native identity.
        trackerWindow = TrackerWindowChrome.makeWindow()
        splitController = TrackerSplitViewController(store: store, presentation: presentation,
                                                    window: trackerWindow, openSettings: openSettings)
        super.init(window: trackerWindow)
        trackerWindow.contentViewController = splitController
        trackerWindow.setContentSize(NSSize(width: 1100, height: 760))
        trackerWindow.delegate = self
        trackerWindow.center()
    }

    required init?(coder: NSCoder) {
        fatalError("TrackerWindowController requires a TrackerStore")
    }

    func present() {
        if trackerWindow.isMiniaturized { trackerWindow.deminiaturize(nil) }
        showWindow(nil)
        trackerWindow.makeKeyAndOrderFront(nil)
        if #available(macOS 14, *) {
            NSApplication.shared.activate()
        } else {
            NSApplication.shared.activate(ignoringOtherApps: true)
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        sender.attachedSheet == nil && presentation.canClose(sender)
    }

    func windowWillClose(_ notification: Notification) {
        splitController.tearDown()
        onClose?()
        presentation.windowClosed(trackerWindow)
    }
}
