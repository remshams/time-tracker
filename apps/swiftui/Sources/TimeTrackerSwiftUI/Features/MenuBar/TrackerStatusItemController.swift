import AppKit
import Combine
import SwiftUI
import TrackerClient

@MainActor
final class TrackerStatusItemController: NSObject, NSPopoverDelegate {
    private let store: TrackerStore
    private let showTracker: () -> Void
    private let quit: () -> Void
    private var statusItem: NSStatusItem?
    private var popover: NSPopover?
    private var subscriptions = Set<AnyCancellable>()
    private var appearanceObservation: NSKeyValueObservation?
    private var menuIsOpen = false

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
            button.sendAction(on: [.leftMouseUp, .rightMouseUp])
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
        if let popover {
            popover.close()
            finishClosing(popover)
        }
        subscriptions.removeAll()
        appearanceObservation = nil
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
        let event = NSApplication.shared.currentEvent
        if event?.type == .rightMouseUp || event?.modifierFlags.contains(.control) == true {
            if let popover { closePopup(popover) }
            else { showPopup() }
        } else {
            if let popover { closePopup(popover) }
            if case .openMenu = store.performMenuPrimaryAction() { showPopup() }
        }
    }

    @objc private func accessibilityOpenMenu() -> Bool {
        guard statusItem != nil else { return false }
        if popover == nil { showPopup() }
        return popover?.isShown == true
    }

    private func showPopup() {
        guard popover == nil, let button = statusItem?.button, button.window != nil else { return }
        let popup = NSPopover()
        popup.behavior = .transient
        popup.animates = false
        popup.delegate = self
        let hosting = NSHostingController(rootView: TrackerMenuPopup(
            store: store, showTracker: showTracker, quit: quit,
            close: { [weak self, weak popup] in
                guard let popup else { return }
                self?.closePopup(popup)
            }))
        hosting.sizingOptions = [.standardBounds, .preferredContentSize]
        popup.contentViewController = hosting
        popover = popup
        NSApplication.shared.activate()
        popup.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        if !popup.isShown { finishClosing(popup) }
    }

    private func closePopup(_ popup: NSPopover) {
        guard popover === popup else { return }
        popup.performClose(nil)
        if !popup.isShown { finishClosing(popup) }
    }

    func popoverDidShow(_ notification: Notification) {
        guard let popup = notification.object as? NSPopover, popover === popup, !menuIsOpen else { return }
        menuIsOpen = true
        statusItem?.button?.highlight(true)
        store.menuOpened()
        popup.contentViewController?.view.window?.makeKey()
    }

    func popoverDidClose(_ notification: Notification) {
        guard let popup = notification.object as? NSPopover else { return }
        finishClosing(popup)
    }

    private func finishClosing(_ popup: NSPopover) {
        guard popover === popup else { return }
        popover = nil
        popup.delegate = nil
        popup.contentViewController = nil
        statusItem?.button?.highlight(false)
        if menuIsOpen {
            menuIsOpen = false
            store.menuClosed()
        }
    }
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
