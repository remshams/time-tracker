import AppKit
import Combine
import TrackerClient

@MainActor
final class TrackerStatusItemController: NSObject {
    private let store: TrackerStore
    private var statusItem: NSStatusItem?
    private var subscriptions = Set<AnyCancellable>()
    private var appearanceObservation: NSKeyValueObservation?
    private var openWindow: (() -> Void)?
    private var showingMenu = false

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
        updateLabel()
    }

    func stop() {
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
        button.setAccessibilityHelp("\(label.help)\nClick to start or stop tracking. Right-click to open the menu.")
    }

    @objc private func clicked() {
        guard !showingMenu else { return }
        let event = NSApplication.shared.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            showMenu()
        } else if case .openMenu = store.performMenuPrimaryAction() {
            showMenu()
        }
    }

    private func showMenu() {
        guard !showingMenu, let button = statusItem?.button else { return }
        showingMenu = true
        store.menuOpened()
        button.highlight(true)
        defer {
            button.highlight(false)
            showingMenu = false
            store.menuClosed()
        }
        // Freeze the presentation for the entire synchronous AppKit tracking loop.
        let menu = makeMenu(store.menu.content, appearance: NSApplication.shared.effectiveAppearance)
        // In flipped views, maxY is the bottom edge beneath the menu bar.
        let bottomY = button.isFlipped ? button.bounds.maxY : button.bounds.minY
        let anchor = NSPoint(x: button.bounds.minX, y: bottomY)
        menu.popUp(positioning: nil, at: anchor, in: button)
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
