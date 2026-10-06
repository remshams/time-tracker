import AppKit
import Combine
import SwiftUI
import TrackerClient

@MainActor
final class TrackerStatusItemController: NSObject, NSWindowDelegate {
    private let store: TrackerStore
    private let showTracker: () -> Void
    private let quit: () -> Void
    private var statusItem: NSStatusItem?
    private var panel: TrackerMenuPanel?
    private var subscriptions = Set<AnyCancellable>()
    private var panelSubscriptions = Set<AnyCancellable>()
    private var appearanceObservation: NSKeyValueObservation?
    private var panelSizeObservation: NSKeyValueObservation?
    private var localMouseMonitor: Any?
    private var globalMouseMonitor: Any?
    private var menuIsOpen = false
    private var controlClickInProgress = false
    private var handledMenuMouseDown: (number: Int, timestamp: TimeInterval)?

    init(store: TrackerStore, showTracker: @escaping () -> Void, quit: @escaping () -> Void) {
        self.store = store
        self.showTracker = showTracker
        self.quit = quit
        super.init()
    }

    func start() {
        guard statusItem == nil else { return }
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem = item
        if let button = item.button {
            button.target = self
            button.action = #selector(clicked)
            button.sendAction(on: [.leftMouseDown, .leftMouseUp, .rightMouseDown])
            button.imagePosition = .imageLeading
            button.setAccessibilityCustomActions([
                NSAccessibilityCustomAction(name: "Open tracking menu", target: self,
                                            selector: #selector(accessibilityOpenMenu))
            ])
            appearanceObservation = button.observe(\.effectiveAppearance) { [weak self] _, _ in
                MainActor.assumeIsolated { self?.updateLabel() }
            }
        }
        store.menu.label.objectWillChange.sink { [weak self] _ in
            MainActor.assumeIsolated { self?.updateLabel() }
        }.store(in: &subscriptions)
        updateLabel()
    }

    func stop() {
        if let panel { closePopup(panel) }
        subscriptions.removeAll()
        appearanceObservation = nil
        controlClickInProgress = false
        handledMenuMouseDown = nil
        if let statusItem {
            statusItem.button?.target = nil
            statusItem.button?.action = nil
            statusItem.button?.setAccessibilityCustomActions(nil)
            NSStatusBar.system.removeStatusItem(statusItem)
        }
        statusItem = nil
    }

    private func updateLabel() {
        guard let button = statusItem?.button else { return }
        let label = store.menu.label.content
        button.image = StatusTaskDotImage.make(indicator: label.indicator,
                                              appearance: button.effectiveAppearance)
        panel?.appearance = button.effectiveAppearance
        button.attributedTitle = NSAttributedString(string: label.totalText.map { " \($0)" } ?? "",
                                                    attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
            .foregroundColor: NSColor.labelColor
        ])
        button.toolTip = label.help
        button.setAccessibilityLabel(label.status)
        button.setAccessibilityValue(label.totalText ?? "")
        button.setAccessibilityHelp("\(label.help)\nClick to start or stop tracking. Right-click or Control-click to open the menu.")
    }

    @objc private func clicked() {
        guard statusItem != nil else { return }
        if let event = NSApplication.shared.currentEvent {
            switch event.type {
            case .leftMouseDown:
                controlClickInProgress = event.modifierFlags.contains(.control)
                guard controlClickInProgress else { return }
                handleMenuMouseDown(event)
                return
            case .rightMouseDown:
                controlClickInProgress = false
                handleMenuMouseDown(event)
                return
            case .leftMouseUp:
                if controlClickInProgress {
                    controlClickInProgress = false
                    return
                }
            default: break
            }
        }
        if let panel { closePopup(panel) }
        if case .openMenu = store.performMenuPrimaryAction() { showPopup() }
    }

    private func handleMenuMouseDown(_ event: NSEvent) {
        if let handledMenuMouseDown, handledMenuMouseDown.number == event.eventNumber,
           handledMenuMouseDown.timestamp == event.timestamp {
            self.handledMenuMouseDown = nil
            return
        }
        handledMenuMouseDown = nil
        togglePopup()
    }

    private func togglePopup() {
        if let panel { closePopup(panel) }
        else { showPopup() }
    }

    @objc private func accessibilityOpenMenu() -> Bool {
        guard statusItem != nil else { return false }
        if panel == nil { showPopup() }
        return panel?.isVisible == true
    }

    private func showPopup() {
        guard panel == nil, let button = statusItem?.button,
              let anchor = statusButtonScreenFrame(), let screen = button.window?.screen else { return }
        let frame = TrackerMenuPanelPlacement.frame(contentSize: CGSize(width: 360, height: 560),
                                                   anchor: anchor, visibleFrame: screen.visibleFrame)
        guard frame.width > 0, frame.height > 0 else { return }
        let popup = TrackerMenuPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel],
                                     backing: .buffered, defer: false)
        popup.isReleasedWhenClosed = false
        popup.isOpaque = false
        popup.backgroundColor = .clear
        popup.hasShadow = true
        popup.isMovable = false
        popup.isFloatingPanel = true
        popup.hidesOnDeactivate = false
        popup.becomesKeyOnlyIfNeeded = false
        popup.isExcludedFromWindowsMenu = true
        popup.level = .popUpMenu
        popup.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
        popup.animationBehavior = .none
        popup.appearance = button.effectiveAppearance
        popup.title = "Time Tracker menu"
        popup.delegate = self
        let shape = RoundedRectangle(cornerRadius: 12)
        let hosting = NSHostingController(rootView: TrackerMenuPopup(
            store: store, showTracker: showTracker, quit: quit,
            close: { [weak self, weak popup] in
                guard let popup else { return }
                self?.closePopup(popup)
            }, width: frame.width, maximumHeight: frame.height)
            .background(.regularMaterial, in: shape)
            .clipShape(shape))
        hosting.sizingOptions = [.intrinsicContentSize, .preferredContentSize]
        popup.contentViewController = hosting
        panel = popup
        resizePopup(popup, to: hosting.sizeThatFits(in: frame.size))
        panelSizeObservation = hosting.observe(\.preferredContentSize, options: [.new]) { [weak self, weak popup] _, change in
            MainActor.assumeIsolated {
                guard let popup, let size = change.newValue else { return }
                self?.resizePopup(popup, to: size)
            }
        }
        installDismissalHandlers(for: popup)
        menuIsOpen = true
        button.highlight(true)
        store.menuOpened()
        popup.makeKeyAndOrderFront(nil)
        if !popup.isVisible { closePopup(popup) }
    }

    private func installDismissalHandlers(for popup: TrackerMenuPanel) {
        let mouseEvents: NSEvent.EventTypeMask = [.leftMouseDown, .rightMouseDown, .otherMouseDown]
        localMouseMonitor = NSEvent.addLocalMonitorForEvents(matching: mouseEvents) { [weak self, weak popup] event in
            MainActor.assumeIsolated {
                guard let self, let popup, self.panel === popup else { return event }
                let isStatusClick = self.isStatusButtonEvent(event)
                if isStatusClick && (event.type == .rightMouseDown ||
                                    event.type == .leftMouseDown && event.modifierFlags.contains(.control)) {
                    self.controlClickInProgress = event.type == .leftMouseDown
                    self.handledMenuMouseDown = (event.eventNumber, event.timestamp)
                    self.togglePopup()
                    return event
                }
                guard event.window !== popup, !isStatusClick else { return event }
                self.closePopup(popup)
                return event
            }
        }
        globalMouseMonitor = NSEvent.addGlobalMonitorForEvents(matching: mouseEvents) { [weak self, weak popup] event in
            MainActor.assumeIsolated {
                guard let self, let popup, self.panel === popup,
                      !self.isStatusButtonEvent(event) else { return }
                self.closePopup(popup)
            }
        }
        for name in [NSApplication.didResignActiveNotification,
                     NSApplication.didChangeScreenParametersNotification] {
            NotificationCenter.default.publisher(for: name)
                .receive(on: RunLoop.main)
                .sink { [weak self, weak popup] _ in
                    MainActor.assumeIsolated {
                        guard let popup else { return }
                        self?.closePopup(popup)
                    }
                }.store(in: &panelSubscriptions)
        }
        for name in [NSWorkspace.didActivateApplicationNotification,
                     NSWorkspace.activeSpaceDidChangeNotification] {
            NSWorkspace.shared.notificationCenter.publisher(for: name)
                .receive(on: RunLoop.main)
                .sink { [weak self, weak popup] _ in
                    MainActor.assumeIsolated {
                        guard let popup else { return }
                        self?.closePopup(popup)
                    }
                }.store(in: &panelSubscriptions)
        }
    }

    func windowDidResignKey(_ notification: Notification) {
        guard let popup = notification.object as? TrackerMenuPanel, panel === popup else { return }
        closePopup(popup)
    }

    private func resizePopup(_ popup: TrackerMenuPanel, to size: CGSize) {
        guard panel === popup, size.width.isFinite, size.height.isFinite,
              size.width > 0, size.height > 0, let anchor = statusButtonScreenFrame(),
              let screen = statusItem?.button?.window?.screen else { return }
        let boundedSize = CGSize(width: min(size.width, 360), height: min(size.height, 560))
        let frame = TrackerMenuPanelPlacement.frame(contentSize: boundedSize, anchor: anchor,
                                                   visibleFrame: screen.visibleFrame)
        guard popup.frame != frame else { return }
        popup.setFrame(frame, display: true)
        popup.invalidateShadow()
    }

    private func statusButtonScreenFrame() -> CGRect? {
        guard let button = statusItem?.button, let window = button.window else { return nil }
        return window.convertToScreen(button.convert(button.bounds, to: nil))
    }

    private func isStatusButtonEvent(_ event: NSEvent) -> Bool {
        switch event.type {
        case .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp:
            guard let anchor = statusButtonScreenFrame() else { return false }
            let point: CGPoint
            if let window = event.window {
                point = window.convertToScreen(CGRect(origin: event.locationInWindow, size: .zero)).origin
            } else { point = event.locationInWindow }
            return anchor.contains(point)
        default: return false
        }
    }

    private func closePopup(_ popup: TrackerMenuPanel) {
        guard panel === popup else { return }
        panel = nil
        panelSizeObservation = nil
        panelSubscriptions.removeAll()
        if let localMouseMonitor { NSEvent.removeMonitor(localMouseMonitor) }
        if let globalMouseMonitor { NSEvent.removeMonitor(globalMouseMonitor) }
        localMouseMonitor = nil
        globalMouseMonitor = nil
        popup.delegate = nil
        popup.orderOut(nil)
        popup.contentViewController = nil
        popup.close()
        statusItem?.button?.highlight(false)
        if menuIsOpen {
            menuIsOpen = false
            store.menuClosed()
        }
    }
}

@MainActor
private final class TrackerMenuPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}

private enum StatusTaskDotImage {
    static func make(indicator: TaskIndicator, appearance: NSAppearance) -> NSImage {
        var color = indicator.color.map(nativeColor) ?? .secondaryLabelColor
        appearance.performAsCurrentDrawingAppearance {
            color = color.usingColorSpace(.deviceRGB) ?? color
        }
        let drawingColor = color
        let image = NSImage(size: NSSize(width: 16, height: 16), flipped: false) { rect in
            let circle = NSBezierPath(ovalIn: rect.insetBy(dx: 3, dy: 3))
            if indicator.isRunning {
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

    private static func nativeColor(_ color: TaskColor) -> NSColor {
        switch color {
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
