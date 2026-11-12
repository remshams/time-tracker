import AppKit
import TrackerClient

/// Owns the status item: it draws the button, opens and closes the menu, updates its live values, and runs the
/// commands its items send.
final class TrackerStatusItemController: NSObject {
    private let sync: TrackerSync
    private let tracking: TrackingStore
    private let menuBar: MenuBarStore
    private let connection: ConnectionStore
    private let menu: TrackerMenuPresentationObserver
    private let preferences: MenuBarPreferencesStore
    private var statusItem: NSStatusItem?
    private var appearanceObservation: NSKeyValueObservation?
    private var openWindow: (() -> Void)?
    private var shownMenu: TrackerStatusMenu?
    private var shortcut: GlobalMenuShortcutHandler?
    /// The app that was in front when the shortcut opened the menu. Focus goes back to it after the menu closes.
    private var previousApplication: NSRunningApplication?

    init(
        sync: TrackerSync, tracking: TrackingStore, menuBar: MenuBarStore, connection: ConnectionStore,
        menu: TrackerMenuPresentationObserver, preferences: MenuBarPreferencesStore
    ) {
        self.sync = sync
        self.tracking = tracking
        self.menuBar = menuBar
        self.connection = connection
        self.menu = menu
        self.preferences = preferences
        super.init()
    }

    func start(openWindow: @escaping () -> Void) {
        self.openWindow = openWindow
        guard statusItem == nil else { return }
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem = item
        if let button = item.button {
            button.setAccessibilityIdentifier("menu.status")
            button.target = self
            button.action = #selector(clicked)
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            button.imagePosition = .imageLeading
            appearanceObservation = button.observe(\.effectiveAppearance) { [weak self] _, _ in
                MainActor.assumeIsolated { self?.updateLabel() }
            }
        }
        menu.onLabelChange = { [weak self] in self?.updateLabel() }
        menu.onValuesChange = { [weak self] in
            guard let self else { return }
            updateMenuValues(menu.values)
        }
        shortcut = GlobalMenuShortcutHandler(preferences: preferences) { [weak self] in self?.toggleMenu() }
        updateLabel()
    }

    private func updateLabel() {
        guard let button = statusItem?.button else { return }
        let label = menu.label
        button.image = TaskDotImage.make(
            color: label.indicator.color,
            isRunning: label.indicator.isRunning,
            appearance: button.effectiveAppearance)
        let total = label.totalText.map { " \($0)" } ?? ""
        button.attributedTitle = NSAttributedString(
            string: total,
            attributes: [
                .font: NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
                .foregroundColor: NSColor.labelColor,
            ])
        button.toolTip = label.help
        button.setAccessibilityLabel(label.status)
        button.setAccessibilityValue(label.totalText ?? "")
        button.setAccessibilityHelp(
            "\(label.help)\nClick to start or stop tracking. Right-click or use the Open menu shortcut to open the menu."
        )
    }

    @objc private func clicked() {
        guard shownMenu == nil else { return }
        let event = NSApplication.shared.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            showNativeMenu()
        } else if case .openMenu = menuBar.performMenuPrimaryAction() {
            showNativeMenu()
        }
    }

    private func toggleMenu() {
        if let shownMenu {
            shownMenu.menu.cancelTracking()
        } else {
            let frontmost = NSWorkspace.shared.frontmostApplication
            previousApplication = frontmost == NSRunningApplication.current ? nil : frontmost
            // The menu receives arrow keys and Return only while the app is active.
            NSApplication.shared.activate(ignoringOtherApps: true)
            showNativeMenu()
        }
    }

    /// Gives focus back to the app that was in front before the shortcut, unless the user chose Open Time Tracker
    /// or moved to another app.
    private func returnFocus() {
        guard let application = previousApplication else { return }
        previousApplication = nil
        guard NSApplication.shared.isActive, !application.isTerminated, !application.isHidden else { return }
        NSApplication.shared.yieldActivation(to: application)
        _ = application.activate(from: .current)
    }

    private func showNativeMenu() {
        guard shownMenu == nil, let statusItem, let button = statusItem.button else { return }
        // Showing the menu refreshes the clock values first, so the menu captures current values even when no
        // display tick ran.
        sync.setMenuVisible(true)
        self.menu.menuOpened()
        let builder = TrackerStatusMenuBuilder(
            menuBar: menuBar, connection: connection.settings, target: self, itemAction: #selector(selected(_:)))
        let built = builder.makeMenu(self.menu.content, appearance: NSApplication.shared.effectiveAppearance)
        let menu = built.menu
        shownMenu = built
        updateMenuValues(self.menu.values)
        button.highlight(true)
        shortcut?.beginMenuTracking { [weak menu] in menu?.cancelTracking() }
        defer {
            statusItem.menu = nil
            button.target = self
            button.action = #selector(clicked)
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            self.shownMenu = nil
            button.highlight(false)
            sync.setMenuVisible(false)
            self.menu.menuClosed()
            shortcut?.endMenuTracking()
            // Wait one main loop turn so a command the menu sends after it closes can keep focus here.
            if previousApplication != nil {
                DispatchQueue.main.async { [weak self] in self?.returnFocus() }
            }
        }
        // AppKit positions status-item menus below the menu bar, including their outer padding.
        statusItem.menu = menu
        button.performClick(nil)
    }

    private func updateMenuValues(_ values: TrackerMenuValues) {
        guard let shownMenu else { return }
        shownMenu.elapsedField?.update(values.elapsedText ?? "Unavailable")
        shownMenu.totalField?.update(values.totalText, help: values.totalsExplanation)
        for (taskID, row) in shownMenu.taskRows {
            guard let duration = values.taskDurationTexts[taskID] else { continue }
            row.update(duration: duration, help: values.totalsExplanation)
        }
    }

    private func copyValue(_ action: MenuShortcutAction, taskID: String, connection: ConnectionSettings) {
        guard let value = menuBar.menuCopyValue(action, taskID: taskID, connection: connection) else {
            NSSound.beep()
            return
        }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        if !pasteboard.setString(value, forType: .string) { NSSound.beep() }
    }

    @objc private func selected(_ item: NSMenuItem) {
        guard item.isEnabled, let command = item.representedObject as? TrackerMenuCommand else { return }
        // `TrackingStore` also validates the captured task and worklog against the current state.
        switch command.action {
        case .start(let id):
            guard command.connection == connection.settings else { return }
            tracking.startTracking(taskID: id)
        case .stop(let id):
            guard command.connection == connection.settings else { return }
            tracking.stopTracking(worklogID: id)
        case .copy(let copyAction, let taskID):
            copyValue(copyAction, taskID: taskID, connection: command.connection)
        case .openWindow:
            previousApplication = nil
            NSApplication.shared.activate(ignoringOtherApps: true)
            openWindow?()
        case .quit: NSApplication.shared.terminate(nil)
        }
    }
}
