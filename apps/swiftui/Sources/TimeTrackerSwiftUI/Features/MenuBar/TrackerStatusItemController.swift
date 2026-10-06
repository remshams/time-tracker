import AppKit
import Combine
import Carbon
import TrackerClient

@MainActor
final class TrackerStatusItemController: NSObject, NSMenuDelegate {
    private let store: TrackerStore
    private var statusItem: NSStatusItem?
    private var subscriptions = Set<AnyCancellable>()
    private var appearanceObservation: NSKeyValueObservation?
    private var openWindow: (() -> Void)?
    private var nativeMenu: NSMenu?
    private var menuContent: TrackerMenuContent?
    private var menuConnection: ConnectionSettings?
    private var openMenus: [NSMenu] = []
    private var keyMonitor: NativeMenuKeyboardMonitor?
    private var keyRouter = MenuTrackingKeyRouter()
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
        keyMonitor?.stop()
        nativeMenu?.cancelTracking()
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
        let event = NSApplication.shared.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            showNativeMenu()
        } else if case .openMenu = store.performMenuPrimaryAction() {
            showNativeMenu()
        }
    }

    private func toggleMenu() {
        if let nativeMenu { nativeMenu.cancelTracking() }
        else {
            NSApplication.shared.activate(ignoringOtherApps: true)
            showNativeMenu()
        }
    }

    private func showNativeMenu() {
        guard nativeMenu == nil, let button = statusItem?.button else { return }
        store.menuOpened()
        let content = store.menu.content
        let menu = makeMenu(content, appearance: NSApplication.shared.effectiveAppearance)
        nativeMenu = menu
        menuContent = content
        menuConnection = store.connectionSettings
        openMenus = [menu]
        keyRouter.reset()
        let monitor = NativeMenuKeyboardMonitor(
            route: { [weak self] event in self?.routeKey(event) ?? .passThrough },
            perform: { [weak self] action in self?.performKeyAction(action) }
        )
        keyMonitor = monitor
        monitor.start()
        button.highlight(true)
        defer {
            monitor.stop()
            keyMonitor = nil
            keyRouter.reset()
            openMenus.removeAll()
            nativeMenu = nil
            menuContent = nil
            menuConnection = nil
            button.highlight(false)
            store.menuClosed()
        }
        let bottomY = button.isFlipped ? button.bounds.maxY : button.bounds.minY
        let anchor = NSPoint(x: button.bounds.minX, y: bottomY)
        menu.popUp(positioning: nil, at: anchor, in: button)
    }

    func menuWillOpen(_ menu: NSMenu) {
        guard nativeMenu != nil, !openMenus.contains(where: { $0 === menu }) else { return }
        openMenus.append(menu)
    }

    func menuDidClose(_ menu: NSMenu) {
        if let index = openMenus.firstIndex(where: { $0 === menu }) {
            openMenus.removeSubrange(index...)
        }
        if menu === nativeMenu { keyMonitor?.stop() }
    }

    private func routeKey(_ event: NSEvent) -> MenuTrackingKeyResult {
        guard nativeMenu != nil, !openMenus.isEmpty else { return .passThrough }
        let shortcut = MenuShortcut(event: event)
        return keyRouter.route(keyCode: event.keyCode, key: shortcut?.key,
                               modifiers: shortcut?.modifiers ?? [],
                               phase: event.type == .keyUp ? .up : .down,
                               isRepeat: event.isARepeat, shortcuts: store.menuShortcuts)
    }

    private func performKeyAction(_ action: MenuShortcutAction) {
        switch action {
        case .openMenu: nativeMenu?.cancelTracking()
        case .copyName, .copyExact, .copyRounded: copyValue(action)
        case .moveDown, .moveUp: break
        }
    }

    private func copyValue(_ action: MenuShortcutAction) {
        guard let item = openMenus.last?.highlightedItem,
              let actionIdentity = item.representedObject as? MenuAction,
              case .start(let id) = actionIdentity,
              let connection = menuConnection,
              let value = store.menuCopyValue(action, taskID: id, connection: connection) else {
            NSSound.beep()
            return
        }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        if !pasteboard.setString(value, forType: .string) { NSSound.beep() }
    }

    private func makeMenu(_ content: TrackerMenuContent, appearance: NSAppearance) -> NSMenu {
        let menu = NSMenu()
        menu.delegate = self
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
            submenu.delegate = self
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
        // All task rows can be highlighted for copying; activation is checked separately.
        let item = addAction(title, action: .start(entry.id), to: menu)
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
        item.toolTip = entry.canStart ? help : "Highlight this task and use a copy shortcut. Tracking is unavailable."
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
        case .start(let id):
            guard menuConnection == store.connectionSettings,
                  let content = menuContent,
                  (content.todayTasks + content.otherTasks).contains(where: { $0.id == id && $0.canStart }) else { return }
            store.startTracking(taskID: id)
        case .stop(let id):
            guard menuConnection == store.connectionSettings,
                  menuContent?.canStopTracking == true else { return }
            store.stopTracking(worklogID: id)
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
