import AppKit
import Combine
import Carbon
import TrackerClient

@MainActor
final class TrackerStatusItemController: NSObject {
    private let store: TrackerStore
    private var statusItem: NSStatusItem?
    private var subscriptions = Set<AnyCancellable>()
    private var appearanceObservation: NSKeyValueObservation?
    private var openWindow: (() -> Void)?
    private var nativeMenu: NSMenu?
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
        guard nativeMenu == nil, let statusItem, let button = statusItem.button else { return }
        store.menuOpened()
        let content = store.menu.content
        let menu = makeMenu(content, appearance: NSApplication.shared.effectiveAppearance)
        nativeMenu = menu
        button.highlight(true)
        defer {
            statusItem.menu = nil
            button.target = self
            button.action = #selector(clicked)
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
            nativeMenu = nil
            button.highlight(false)
            store.menuClosed()
        }
        // AppKit positions status-item menus below the menu bar, including their outer padding.
        statusItem.menu = menu
        button.performClick(nil)
    }

    private func copyValue(_ action: MenuShortcutAction, taskID: String, connection: ConnectionSettings) {
        guard let value = store.menuCopyValue(action, taskID: taskID, connection: connection) else {
            NSSound.beep()
            return
        }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        if !pasteboard.setString(value, forType: .string) { NSSound.beep() }
    }

    private func makeMenu(_ content: TrackerMenuContent, appearance: NSAppearance) -> NSMenu {
        let menu = NSMenu()
        menu.appearance = appearance
        menu.autoenablesItems = false
        let connection = store.connectionSettings
        let serverURL = connection.mode == .server
            ? connection.serverURL.trimmingCharacters(in: .whitespacesAndNewlines) : nil
        addText(content.connectionStatusText, value: serverURL, to: menu)
        if content.isStale { addText("Showing last confirmed state", to: menu) }
        addText(content.runningTaskName, value: content.elapsedText, to: menu)
        if let status = content.autoPauseStatusText { addText(status, to: menu) }
        if let error = content.trackingError { addText(error, to: menu) }
        menu.addItem(.separator())
        addText(content.dailyTotalsStatus == .cached ? "Today, cached" : "Today", to: menu)
        addText("Total", value: content.totalText, to: menu, help: content.totalsExplanation)
        menu.addItem(.separator())
        for entry in content.todayTasks {
            let archived = entry.task.archived ? "  Archived" : ""
            addTask(entry, title: "\(entry.task.name)  \(entry.durationText)\(archived)",
                    help: content.totalsExplanation, content: content,
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
                addTask(entry, title: entry.task.name, help: nil, content: content,
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
                         content: TrackerMenuContent, appearance: NSAppearance, to menu: NSMenu) {
        let isRunning = entry.isRunning && !content.isStale
        let trackingAction = entry.trackingAction(in: content).map { action -> MenuAction in
            switch action {
            case .start(let taskID): return .start(taskID)
            case .stop(let worklogID): return .stop(worklogID)
            }
        }
        let submenu = NSMenu(title: entry.task.name)
        submenu.appearance = appearance
        submenu.autoenablesItems = false
        addAction(entry.isRunning ? "Stop tracking" : "Start tracking",
                  action: trackingAction, enabled: trackingAction != nil, to: submenu)
        submenu.addItem(.separator())
        for action in [MenuShortcutAction.copyName, .copyExact, .copyRounded] {
            let available = store.menuCopyValue(action, taskID: entry.id,
                                                connection: store.connectionSettings) != nil
            addAction(action.title, action: .copy(action, entry.id), enabled: available, to: submenu)
        }
        let item = addAction(title, action: trackingAction, to: menu)
        item.submenu = submenu
        if trackingAction != nil {
            // Setting a submenu assigns submenuAction; restore the row's tracking action afterward.
            item.target = self
            item.action = #selector(selected(_:))
        }
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
        item.toolTip = trackingAction != nil ? help : "Open the submenu to copy this task's name or time. Tracking is unavailable."
        let status: String
        if content.isStale {
            status = entry.isRunning ? "last confirmed tracking, current status unavailable"
                : "current tracking status unavailable"
        } else { status = entry.isRunning ? "tracking" : "not tracking" }
        item.setAccessibilityLabel("\(title), \(status)")
    }

    private func addText(_ title: String, value: String? = nil, to menu: NSMenu, help: String? = nil) {
        let text = value.map { "\(title), \($0)" } ?? title
        let item = NSMenuItem(title: text, action: nil, keyEquivalent: "")
        item.isEnabled = false
        let label = NSTextField(labelWithString: title)
        label.font = .menuFont(ofSize: 0)
        label.textColor = .labelColor
        label.lineBreakMode = .byTruncatingMiddle
        label.toolTip = help ?? title
        label.setAccessibilityLabel(title)
        let valueLabel = value.map { NSTextField(labelWithString: $0) }
        valueLabel?.font = label.font
        valueLabel?.textColor = .labelColor
        valueLabel?.alignment = .right
        valueLabel?.lineBreakMode = .byTruncatingMiddle
        valueLabel?.toolTip = help ?? value
        valueLabel?.setAccessibilityLabel(value ?? "")
        let valueSize = valueLabel?.fittingSize ?? .zero
        let width = min(480, label.fittingSize.width + valueSize.width + (value == nil ? 40 : 56))
        let height = max(label.fittingSize.height, valueSize.height) + 8
        let view = NSView(frame: NSRect(x: 0, y: 0, width: width, height: height))
        view.appearance = menu.appearance
        view.autoresizingMask = [.width]
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 20),
            label.centerYAnchor.constraint(equalTo: view.centerYAnchor)
        ])
        if let valueLabel {
            valueLabel.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(valueLabel)
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            NSLayoutConstraint.activate([
                valueLabel.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20),
                valueLabel.centerYAnchor.constraint(equalTo: view.centerYAnchor),
                valueLabel.widthAnchor.constraint(lessThanOrEqualTo: view.widthAnchor, multiplier: 0.65),
                label.trailingAnchor.constraint(lessThanOrEqualTo: valueLabel.leadingAnchor, constant: -16)
            ])
        } else {
            label.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20).isActive = true
        }
        item.view = view
        menu.addItem(item)
    }

    @discardableResult
    private func addAction(_ title: String, action: MenuAction?, enabled: Bool = true,
                           to menu: NSMenu) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(selected(_:)), keyEquivalent: "")
        item.target = self
        item.representedObject = action.map { MenuCommand(action: $0, connection: store.connectionSettings) }
        item.isEnabled = enabled
        menu.addItem(item)
        return item
    }

    private enum MenuAction {
        case start(String), stop(String), copy(MenuShortcutAction, String), openWindow, quit
    }

    private struct MenuCommand {
        let action: MenuAction
        let connection: ConnectionSettings
    }

    @objc private func selected(_ item: NSMenuItem) {
        guard item.isEnabled, let command = item.representedObject as? MenuCommand else { return }
        // The session also validates the captured task and worklog against its current state.
        switch command.action {
        case .start(let id):
            guard command.connection == store.connectionSettings else { return }
            store.startTracking(taskID: id)
        case .stop(let id):
            guard command.connection == store.connectionSettings else { return }
            store.stopTracking(worklogID: id)
        case .copy(let copyAction, let taskID):
            copyValue(copyAction, taskID: taskID, connection: command.connection)
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
