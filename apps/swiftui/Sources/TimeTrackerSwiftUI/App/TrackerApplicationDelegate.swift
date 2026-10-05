import AppKit
import SwiftUI

@MainActor
final class TrackerApplicationDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation, NSWindowDelegate {
    private lazy var store = TrackerStore()
    private lazy var statusItem = TrackerStatusItemController(store: store)
    private lazy var taskPresentation = TrackerTaskPresentationCoordinator(
        store: store,
        presentingWindow: { [weak self] needsEditor in
            guard let self else { return nil }
            if needsEditor {
                let controller = self.mainWindowController()
                controller.present()
                return controller.trackerWindow
            }
            guard !NSApplication.shared.isHidden else { return nil }
            return self.trackerWindows.first(where: {
                $0.trackerWindow.isVisible && !$0.trackerWindow.isMiniaturized
            })?.trackerWindow
        }
    )
    private var trackerWindows: [TrackerWindowController] = []
    private var settingsController: NSWindowController?

    func applicationWillFinishLaunching(_ notification: Notification) {
        TrackerApplicationMenus.install(for: self)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem.start { [weak self] in self?.showMainWindow(nil) }
        taskPresentation.start()
        showMainWindow(nil)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showMainWindow(nil)
        return false
    }

    func applicationWillTerminate(_ notification: Notification) {
        taskPresentation.stop()
    }

    @objc func showMainWindow(_ sender: Any?) {
        mainWindowController().present()
        taskPresentation.windowAvailable()
    }

    @objc func createWindow(_ sender: Any?) {
        makeWindowController().present()
        taskPresentation.windowAvailable()
    }

    @objc func createTask(_ sender: Any?) {
        guard canCreateTask else { return }
        mainWindowController().present()
        store.creation.open()
    }

    @objc func openSettings(_ sender: Any?) {
        if settingsController == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 540, height: 680),
                styleMask: [.titled, .closable], backing: .buffered, defer: false
            )
            window.title = "Settings"
            window.isReleasedWhenClosed = false
            window.isExcludedFromWindowsMenu = true
            window.delegate = self
            window.center()
            settingsController = NSWindowController(window: window)
        }
        if let window = settingsController?.window, window.contentViewController == nil {
            window.contentViewController = NSHostingController(rootView: ConnectionSettingsView(store: store))
            window.setContentSize(NSSize(width: 540, height: 680))
            window.center()
        }
        settingsController?.showWindow(sender)
        if #available(macOS 14, *) {
            NSApplication.shared.activate()
        } else {
            NSApplication.shared.activate(ignoringOtherApps: true)
        }
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        if menuItem.action == #selector(createTask(_:)) { return canCreateTask }
        return true
    }

    func windowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow, window === settingsController?.window else { return }
        // A fresh settings view on reopen resets its draft and connection result.
        window.contentViewController = nil
    }

    private var canCreateTask: Bool {
        store.creation.canOpen && !store.creation.state.isPresented
    }

    private func mainWindowController() -> TrackerWindowController {
        let application = NSApplication.shared
        if let controller = trackerWindows.first(where: { $0.trackerWindow === application.keyWindow }) {
            return controller
        }
        if let controller = trackerWindows.first(where: { $0.trackerWindow === application.mainWindow }) {
            return controller
        }
        return trackerWindows.last ?? makeWindowController()
    }

    private func makeWindowController() -> TrackerWindowController {
        let controller = TrackerWindowController(store: store, presentation: taskPresentation,
                                                openSettings: { [weak self] in self?.openSettings(nil) })
        controller.onClose = { [weak self, weak controller] in
            guard let self, let controller else { return }
            self.trackerWindows.removeAll { $0 === controller }
        }
        trackerWindows.append(controller)
        return controller
    }
}
