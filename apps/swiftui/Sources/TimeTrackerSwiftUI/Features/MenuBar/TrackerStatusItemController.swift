import AppKit
import Combine
import Carbon
import SwiftUI
import TrackerClient

@MainActor
final class TrackerStatusItemController: NSObject, NSPopoverDelegate {
    private let store: TrackerStore
    private var statusItem: NSStatusItem?
    private var subscriptions = Set<AnyCancellable>()
    private var appearanceObservation: NSKeyValueObservation?
    private var openWindow: (() -> Void)?
    private var popover: NSPopover?
    private var nativeMenu: NSMenu?
    private var openKeyboardMenuAfterNativeClose = false
    private var menuState: MenuDropdownState?
    private var keyMonitor: Any?
    private var shortcutRegistration: GlobalMenuShortcutRegistration?

    init(store: TrackerStore) {
        self.store = store
        super.init()
    }

    func start(openWindow: @escaping () -> Void) {
        self.openWindow = openWindow
        guard statusItem == nil else { return }
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem = item
        if let button = item.button {
            button.target = self
            button.action = #selector(clicked)
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            button.imagePosition = .imageLeading
            appearanceObservation = button.observe(\.effectiveAppearance) { [weak self] _, _ in
                MainActor.assumeIsolated { self?.updateLabel() }
            }
        }
        store.menu.label.objectWillChange.sink { [weak self] _ in
            MainActor.assumeIsolated { self?.updateLabel() }
        }.store(in: &subscriptions)
        NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)
            .sink { [weak self] _ in MainActor.assumeIsolated { self?.stop() } }
            .store(in: &subscriptions)
        let registration = GlobalMenuShortcutRegistration { [weak self] in self?.toggleMenu() }
        shortcutRegistration = registration
        store.registerMenuShortcut = { [weak registration] shortcut in
            do {
                try registration?.register(shortcut)
                return nil
            } catch { return error.localizedDescription }
        }
        do { try registration.register(store.menuShortcuts.openMenu) }
        catch { store.reportMenuGlobalShortcutError(error.localizedDescription) }
        DistributedNotificationCenter.default()
            .publisher(for: Notification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String))
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    do {
                        try self.shortcutRegistration?.register(self.store.menuShortcuts.openMenu)
                        self.store.reportMenuGlobalShortcutError(nil)
                    } catch { self.store.reportMenuGlobalShortcutError(error.localizedDescription) }
                }
            }.store(in: &subscriptions)
        updateLabel()
    }

    func stop() {
        openKeyboardMenuAfterNativeClose = false
        nativeMenu?.cancelTracking()
        closeMenu()
        releaseMenu()
        shortcutRegistration?.unregister()
        shortcutRegistration = nil
        store.registerMenuShortcut = nil
        subscriptions.removeAll()
        appearanceObservation = nil
        if let statusItem { NSStatusBar.system.removeStatusItem(statusItem) }
        statusItem = nil
        openWindow = nil
    }

    private func updateLabel() {
        guard let button = statusItem?.button else { return }
        let label = store.menu.label.content
        button.image = TaskDotImage.make(color: label.indicator.color,
                                        isRunning: label.indicator.isRunning,
                                        appearance: button.effectiveAppearance)
        let total = label.totalText.map { " \($0)" } ?? ""
        button.attributedTitle = NSAttributedString(string: total, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
            .foregroundColor: NSColor.labelColor
        ])
        button.toolTip = label.help
        button.setAccessibilityLabel(label.status)
        button.setAccessibilityHelp("\(label.help)\nClick to start or stop tracking. Right-click or use the Open menu shortcut to open the menu.")
    }

    @objc private func clicked() {
        guard nativeMenu == nil else { return }
        if popover != nil {
            closeMenu()
            return
        }
        let event = NSApplication.shared.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            showNativeMenu()
        } else if case .openMenu = store.performMenuPrimaryAction() {
            showNativeMenu()
        }
    }

    private func toggleMenu() {
        if let nativeMenu {
            openKeyboardMenuAfterNativeClose = true
            nativeMenu.cancelTracking()
            return
        }
        if popover == nil { showMenu() } else { closeMenu() }
    }

    private func showNativeMenu() {
        guard nativeMenu == nil, popover == nil, let button = statusItem?.button else { return }
        store.menuOpened()
        let menu = makeMenu(store.menu.content, appearance: NSApplication.shared.effectiveAppearance)
        nativeMenu = menu
        button.highlight(true)
        defer {
            nativeMenu = nil
            button.highlight(false)
            store.menuClosed()
            let openKeyboardMenu = openKeyboardMenuAfterNativeClose
            openKeyboardMenuAfterNativeClose = false
            if openKeyboardMenu { showMenu() }
        }
        let bottomY = button.isFlipped ? button.bounds.maxY : button.bounds.minY
        let anchor = NSPoint(x: button.bounds.minX, y: bottomY)
        menu.popUp(positioning: nil, at: anchor, in: button)
    }

    private func showMenu() {
        guard nativeMenu == nil, popover == nil, let button = statusItem?.button else { return }
        store.menuOpened()
        let content = store.menu.content
        let state = MenuDropdownState(content: content, connection: store.connectionSettings)
        menuState = state
        let popover = NSPopover()
        self.popover = popover
        popover.behavior = .transient
        popover.delegate = self
        let hostingController = NSHostingController(rootView: MenuDropdownView(
            state: state, shortcuts: store.menuShortcuts,
            activateTask: { [weak self] id in self?.activateTask(id) },
            stopTracking: { [weak self] in self?.stopTracking() },
            copyValue: { [weak self] action in self?.copyValue(action) },
            openWindow: { [weak self] in self?.revealWindow() },
            quit: { NSApplication.shared.terminate(nil) }
        ))
        hostingController.sizingOptions = [.preferredContentSize]
        popover.contentViewController = hostingController
        button.highlight(true)
        NSApplication.shared.activate(ignoringOtherApps: true)
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        guard let window = popover.contentViewController?.view.window else {
            closeMenu()
            releaseMenu()
            return
        }
        window.makeKey()
        window.acceptsMouseMovedEvents = true
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .mouseMoved]) { [weak self, weak window] event in
            MainActor.assumeIsolated {
                guard let self, let window, event.window === window, self.popover?.isShown == true else { return event }
                if event.type == .mouseMoved {
                    self.menuState?.pointerMoved()
                    return event
                }
                return self.handleKey(event) ? nil : event
            }
        }
    }

    func popoverDidClose(_ notification: Notification) { releaseMenu() }

    private func closeMenu() { popover?.performClose(nil) }

    private func releaseMenu() {
        guard popover != nil else { return }
        if let keyMonitor { NSEvent.removeMonitor(keyMonitor) }
        keyMonitor = nil
        popover?.delegate = nil
        popover = nil
        menuState = nil
        statusItem?.button?.highlight(false)
        store.menuClosed()
    }

    private func handleKey(_ event: NSEvent) -> Bool {
        guard let state = menuState else { return false }
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        if modifiers.isEmpty {
            switch event.keyCode {
            case 125: state.moveDown(); return true
            case 126: state.moveUp(); return true
            case 36, 76:
                guard state.allowsTaskActivation else { return false }
                if let id = state.selection.selectedTaskID { activateTask(id) }
                return true
            case 53: closeMenu(); return true
            default: break
            }
        }
        guard let shortcut = MenuShortcut(event: event),
              let action = store.menuShortcuts.action(forKey: shortcut.key, modifiers: shortcut.modifiers) else { return false }
        switch action {
        case .moveDown: state.moveDown()
        case .moveUp: state.moveUp()
        case .copyName, .copyExact, .copyRounded:
            if !event.isARepeat { copyValue(action) }
        case .openMenu: return false
        }
        return true
    }

    private func activateTask(_ id: String) {
        guard let state = menuState,
              state.connection == store.connectionSettings,
              let entry = (state.content.todayTasks + state.content.otherTasks).first(where: { $0.id == id }),
              entry.canStart else { return }
        closeMenu()
        store.startTracking(taskID: id)
    }

    private func stopTracking() {
        guard let state = menuState, state.connection == store.connectionSettings,
              state.content.canStopTracking, let id = state.content.activeWorklogID else { return }
        closeMenu()
        store.stopTracking(worklogID: id)
    }

    private func copyValue(_ action: MenuShortcutAction) {
        guard let state = menuState, let id = state.selection.selectedTaskID else { return }
        guard let value = store.menuCopyValue(action, taskID: id, connection: state.connection) else {
            state.feedback = "This value is unavailable. Reopen the menu to refresh."
            return
        }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        if pasteboard.setString(value, forType: .string) {
            let cached = action != .copyName && store.dailyTotalsStatus == .cached
            state.feedback = "Copied \(value)\(cached ? " from cached totals" : "")"
        } else { state.feedback = "Could not copy to the clipboard." }
    }

    private func revealWindow() {
        closeMenu()
        NSApplication.shared.activate(ignoringOtherApps: true)
        openWindow?()
    }

    private func makeMenu(_ content: TrackerMenuContent, appearance: NSAppearance) -> NSMenu {
        let menu = NSMenu()
        menu.appearance = appearance
        menu.autoenablesItems = false
        addText(content.connectionStatusText, to: menu)
        if content.isStale { addText("Showing last confirmed state", to: menu) }
        addText(content.runningTaskName, to: menu)
        if let status = content.autoPauseStatusText { addText(status, to: menu) }
        if let error = content.trackingError { addText(error, to: menu) }
        if let elapsed = content.elapsedText { addText(elapsed, to: menu) }
        if let id = content.activeWorklogID {
            addAction("Stop tracking", action: .stop(id), enabled: content.canStopTracking, to: menu)
        }
        menu.addItem(.separator())
        addText(content.dailyTotalsStatus == .cached ? "Today, cached" : "Today", to: menu)
        addText("Total today: \(content.totalText)", to: menu, help: content.totalsExplanation)
        for entry in content.todayTasks {
            let archived = entry.task.archived ? "  Archived" : ""
            addTask(entry, title: "\(entry.task.name)  \(entry.durationText)\(archived)",
                    help: content.totalsExplanation, isStale: content.isStale,
                    appearance: appearance, to: menu)
        }
        if content.todayTasks.isEmpty {
            let empty: String
            switch content.dailyTotalsStatus {
            case .current: empty = "No time logged today"
            case .cached: empty = "Last confirmed totals are empty"
            case .loading: empty = "Loading today's totals"
            case .unavailable: empty = "Today's totals are unavailable"
            }
            addText(empty, to: menu)
        }
        if !content.otherTasks.isEmpty {
            let submenu = NSMenu(title: "Start tracking")
            submenu.appearance = appearance
            submenu.autoenablesItems = false
            for entry in content.otherTasks {
                addTask(entry, title: entry.task.name, help: nil, isStale: content.isStale,
                        appearance: appearance, to: submenu)
            }
            let parent = NSMenuItem(title: "Start tracking", action: nil, keyEquivalent: "")
            parent.submenu = submenu
            parent.isEnabled = true
            menu.addItem(parent)
        }
        menu.addItem(.separator())
        addAction("Open Time Tracker", action: .openWindow, to: menu)
        addAction("Quit Time Tracker", action: .quit, to: menu)
        return menu
    }

    private func addTask(_ entry: TrackerMenuTask, title: String, help: String?,
                         isStale: Bool, appearance: NSAppearance, to menu: NSMenu) {
        let isRunning = entry.isRunning && !isStale
        let item = addAction(title, action: .start(entry.id), enabled: entry.canStart || isRunning, to: menu)
        if isRunning {
            item.attributedTitle = NSAttributedString(string: title, attributes: [
                .font: NSFont.boldSystemFont(ofSize: NSFont.menuFont(ofSize: 0).pointSize)
            ])
        }
        item.image = TaskDotImage.make(color: TaskColor.forTaskID(entry.id),
                                      isRunning: isRunning, appearance: appearance)
        if #available(macOS 27.0, *) {
            item.preferredImageVisibility = .visible
        }
        item.toolTip = help
        let status: String
        if isStale {
            status = entry.isRunning ? "last confirmed tracking, current status unavailable"
                : "current tracking status unavailable"
        } else { status = entry.isRunning ? "tracking" : "not tracking" }
        item.setAccessibilityLabel("\(title), \(status)")
    }

    private func addText(_ title: String, to menu: NSMenu, help: String? = nil) {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        let label = NSTextField(labelWithString: title)
        label.font = .menuFont(ofSize: 0)
        label.textColor = .labelColor
        label.lineBreakMode = .byTruncatingMiddle
        label.toolTip = help ?? title
        label.setAccessibilityLabel(title)
        let size = label.fittingSize
        let width = min(480, size.width + 40)
        let view = NSView(frame: NSRect(x: 0, y: 0, width: width, height: size.height + 8))
        view.appearance = menu.appearance
        label.frame = NSRect(x: 20, y: 4, width: width - 40, height: size.height)
        label.autoresizingMask = [.width]
        view.addSubview(label)
        item.view = view
        menu.addItem(item)
    }

    @discardableResult
    private func addAction(_ title: String, action: MenuAction, enabled: Bool = true,
                           to menu: NSMenu) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(selected(_:)), keyEquivalent: "")
        item.target = self
        item.representedObject = action
        item.isEnabled = enabled
        menu.addItem(item)
        return item
    }

    private enum MenuAction { case start(String), stop(String), openWindow, quit }

    @objc private func selected(_ item: NSMenuItem) {
        guard let action = item.representedObject as? MenuAction else { return }
        // The session validates these captured identities against its current snapshot.
        switch action {
        case .start(let id): store.startTracking(taskID: id)
        case .stop(let id): store.stopTracking(worklogID: id)
        case .openWindow:
            NSApplication.shared.activate(ignoringOtherApps: true)
            openWindow?()
        case .quit: NSApplication.shared.terminate(nil)
        }
    }
}

private extension TaskColor {
    var nativeColor: NSColor {
        switch self {
        case .blue: return .systemBlue
        case .teal: return .systemTeal
        case .green: return .systemGreen
        case .orange: return .systemOrange
        case .red: return .systemRed
        case .purple: return .systemPurple
        case .pink: return .systemPink
        }
    }
}

private enum TaskDotImage {
    static func make(color: TaskColor?, isRunning: Bool, appearance: NSAppearance) -> NSImage {
        var resolvedColor = color?.nativeColor ?? .secondaryLabelColor
        appearance.performAsCurrentDrawingAppearance {
            resolvedColor = resolvedColor.usingColorSpace(.deviceRGB) ?? resolvedColor
        }
        let drawingColor = resolvedColor
        let image = NSImage(size: NSSize(width: 16, height: 16), flipped: false) { rect in
            let circle = NSBezierPath(ovalIn: rect.insetBy(dx: 3, dy: 3))
            if isRunning {
                drawingColor.setFill()
                circle.fill()
            } else {
                drawingColor.setStroke()
                circle.lineWidth = 1.8
                circle.stroke()
            }
            return true
        }
        image.isTemplate = false
        return image
    }
}
