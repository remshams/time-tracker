import AppKit
import SwiftUI
import TrackerClient

/// Shows the Quick Track dialog in a floating panel over the current app.
///
/// The panel doesn't activate Time Tracker, so the main window stays where it is and the app in front stays in front.
/// It still becomes the key window, so the search field gets the typed keys. The store decides when the dialog shows;
/// this controller follows `QuickTrackStore.isPresented` and closes the dialog when the panel loses key status, for
/// example after a click into another app.
final class QuickTrackPanelController: NSObject, NSWindowDelegate {
    private let store: QuickTrackStore
    private let panel: QuickTrackPanel
    private let hostingView: QuickTrackHostingView
    /// The top edge of the panel while it shows. The panel grows and shrinks downward from here.
    private var top: CGFloat = 0

    init(store: QuickTrackStore) {
        self.store = store
        panel = QuickTrackPanel()
        hostingView = QuickTrackHostingView(rootView: QuickTrackView(store: store))
        super.init()
        hostingView.sizingOptions = [.intrinsicContentSize]
        hostingView.onSizeChange = { [weak self] in self?.fitToContent() }
        panel.contentView = hostingView
        panel.delegate = self
        panel.commandKeyHandler = { [weak self] event in self?.copy(for: event) ?? false }
        store.onReject = { NSSound.beep() }
        Task { [weak self, store] in
            for await isPresented in Observations({ store.isPresented }) {
                guard let self else { return }
                isPresented ? show() : hide()
            }
        }
    }

    /// The Quick Track shortcut opens the dialog, or closes it while it shows.
    func toggle() {
        store.isPresented ? store.close() : store.open()
    }

    private func show() {
        guard !panel.isVisible else { return }
        let screen = Self.screenWithPointer()
        let visible = screen.visibleFrame
        // Like Spotlight, the dialog sits a little above the middle of the screen.
        top = visible.maxY - visible.height * QuickTrackLayout.topOffsetRatio
        fitToContent(on: visible)
        panel.orderFrontRegardless()
        panel.makeKey()
    }

    private func hide() {
        guard panel.isVisible else { return }
        panel.orderOut(nil)
    }

    private func fitToContent(on visible: NSRect? = nil) {
        let area = visible ?? panel.screen?.visibleFrame ?? Self.screenWithPointer().visibleFrame
        let size = hostingView.fittingSize
        let origin = NSPoint(x: area.midX - size.width / 2, y: top - size.height)
        panel.setFrame(NSRect(origin: origin, size: size), display: true)
    }

    private static func screenWithPointer() -> NSScreen {
        let pointer = NSEvent.mouseLocation
        return NSScreen.screens.first { NSMouseInRect(pointer, $0.frame, false) } ?? NSScreen.main
            ?? NSScreen.screens[0]
    }

    /// Runs a copy action when `event` is ⌘C (task name), ⌘T (exact duration) or ⌘S (rounded duration), and reports
    /// whether it handled the event. ⌘C with text selected in the search field is left to the field, which copies the
    /// text.
    private func copy(for event: NSEvent) -> Bool {
        guard event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
            let key = event.charactersIgnoringModifiers?.lowercased()
        else { return false }
        let action: TaskCopyAction
        switch key {
        case "c":
            guard !hasSelectedSearchText else { return false }
            action = .copyName
        case "t": action = .copyExact
        case "s": action = .copyRounded
        default: return false
        }
        if let value = store.copyValue(action) { TaskPasteboard.copy(value) } else { NSSound.beep() }
        return true
    }

    private var hasSelectedSearchText: Bool {
        guard let editor = panel.firstResponder as? NSTextView else { return false }
        return editor.selectedRange().length > 0
    }

    func windowDidResignKey(_ notification: Notification) {
        // A click outside the dialog or a switch to another app takes the key status away.
        store.close()
    }
}

/// A borderless, non-activating panel that can still become the key window.
private final class QuickTrackPanel: NSPanel {
    init() {
        super.init(
            contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel, .fullSizeContentView],
            backing: .buffered, defer: true)
        // The dialog appears on the current Space, also over an app in full screen.
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient, .ignoresCycle]
        isFloatingPanel = true
        hidesOnDeactivate = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        animationBehavior = .utilityWindow
        setAccessibilityIdentifier("quick-track.panel")
        setAccessibilityLabel("Quick Track")
    }

    /// Gets each key equivalent, such as a key press with ⌘, and returns whether it handled it.
    var commandKeyHandler: ((NSEvent) -> Bool)?

    override var canBecomeKey: Bool { true }

    // AppKit offers a key press with ⌘ to the key window's key equivalents before the main menu, so the copy keys
    // win over Edit > Copy.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if event.type == .keyDown, commandKeyHandler?(event) == true { return true }
        return super.performKeyEquivalent(with: event)
    }
}

/// Reports changes of the SwiftUI content's size, so the panel can follow it.
private final class QuickTrackHostingView: NSHostingView<QuickTrackView> {
    var onSizeChange: (() -> Void)?

    override func invalidateIntrinsicContentSize() {
        super.invalidateIntrinsicContentSize()
        // Resizing the panel during the layout pass that invalidated the size would lay it out again from inside it.
        DispatchQueue.main.async { [weak self] in self?.onSizeChange?() }
    }
}
