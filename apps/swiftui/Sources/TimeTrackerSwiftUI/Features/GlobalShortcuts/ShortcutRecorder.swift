import AppKit
import Carbon
import SwiftUI
import TrackerClient

struct ShortcutRecorder: NSViewRepresentable {
    var shortcut: GlobalShortcut
    var onRecord: (GlobalShortcut) -> Void

    func makeNSView(context: Context) -> ShortcutRecorderButton {
        let button = ShortcutRecorderButton(frame: .zero)
        updateNSView(button, context: context)
        return button
    }

    func updateNSView(_ button: ShortcutRecorderButton, context: Context) {
        button.shortcut = shortcut
        button.onRecord = onRecord
        button.isEnabled = context.environment.isEnabled
        if !button.isEnabled { button.stopRecording() }
        button.updateTitle()
    }

    static func dismantleNSView(_ button: ShortcutRecorderButton, coordinator: ()) {
        button.stopRecording()
        button.onRecord = nil
    }
}

final class ShortcutRecorderButton: NSButton {
    var shortcut: GlobalShortcut?
    var onRecord: ((GlobalShortcut) -> Void)?
    private var isRecording = false
    private var eventMonitor: Any?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        bezelStyle = .rounded
        focusRingType = .exterior
        target = self
        action = #selector(beginRecording)
        toolTip = "Click and press a shortcut. Escape cancels."
        setAccessibilityLabel("Keyboard shortcut")
    }

    required init?(coder: NSCoder) { fatalError("ShortcutRecorderButton requires init(frame:)") }

    isolated deinit {
        if let eventMonitor { NSEvent.removeMonitor(eventMonitor) }
    }

    override var acceptsFirstResponder: Bool { isEnabled }

    override var intrinsicContentSize: NSSize {
        NSSize(width: max(160, super.intrinsicContentSize.width), height: super.intrinsicContentSize.height)
    }

    override func becomeFirstResponder() -> Bool {
        guard super.becomeFirstResponder() else { return false }
        startRecording()
        return true
    }

    override func resignFirstResponder() -> Bool {
        stopRecording()
        return super.resignFirstResponder()
    }

    @objc private func beginRecording() {
        guard isEnabled, window?.makeFirstResponder(self) == true else { return }
        startRecording()
    }

    private func startRecording() {
        isRecording = true
        if eventMonitor == nil {
            // Capture before Settings buttons handle Return or Escape equivalents.
            eventMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                let captured = MainActor.assumeIsolated {
                    guard let self, self.window === event.window else { return false }
                    return self.capture(event)
                }
                return captured ? nil : event
            }
        }
        updateTitle()
    }

    override func keyDown(with event: NSEvent) {
        guard capture(event) else { super.keyDown(with: event); return }
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, isRecording else { return false }
        return capture(event)
    }

    private func capture(_ event: NSEvent) -> Bool {
        guard isEnabled, isRecording, window?.firstResponder === self else { return false }
        if event.keyCode == UInt16(kVK_Escape) {
            finishRecording()
            return true
        }
        if event.keyCode == UInt16(kVK_Tab) {
            if event.modifierFlags.contains(.shift) {
                window?.selectPreviousKeyView(self)
            } else {
                window?.selectNextKeyView(self)
            }
            return true
        }
        guard let shortcut = GlobalShortcut(event: event) else {
            NSSound.beep()
            return true
        }
        finishRecording()
        onRecord?(shortcut)
        return true
    }

    func captureRegisteredShortcut(_ shortcut: GlobalShortcut) -> Bool {
        guard isEnabled, isRecording, window?.firstResponder === self else { return false }
        finishRecording()
        onRecord?(shortcut)
        return true
    }

    private func finishRecording() {
        stopRecording()
        if window?.firstResponder === self { window?.makeFirstResponder(nil) }
    }

    func stopRecording() {
        isRecording = false
        if let eventMonitor { NSEvent.removeMonitor(eventMonitor) }
        eventMonitor = nil
        updateTitle()
    }

    func updateTitle() {
        title = isRecording ? "Press shortcut..." : shortcut?.displayText ?? "Record shortcut"
        setAccessibilityValue(title)
    }
}
