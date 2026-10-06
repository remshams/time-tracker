import AppKit
import Carbon
import SwiftUI
import TrackerClient

extension MenuShortcut {
    init?(event: NSEvent) {
        guard event.type == .keyDown,
              let key = event.characters(byApplyingModifiers: event.modifierFlags.intersection(.shift)),
              key.unicodeScalars.count == 1,
              let scalar = key.unicodeScalars.first,
              (33...126).contains(scalar.value) else { return nil }
        var modifiers: Modifiers = []
        if event.modifierFlags.contains(.command) { modifiers.insert(.command) }
        if event.modifierFlags.contains(.control) { modifiers.insert(.control) }
        if event.modifierFlags.contains(.option) { modifiers.insert(.option) }
        if event.modifierFlags.contains(.shift) { modifiers.insert(.shift) }
        self.init(key: key, modifiers: modifiers)
    }
}

enum GlobalMenuShortcutError: LocalizedError {
    case unsupportedKey(String)
    case missingModifier
    case handlerInstallation(OSStatus)
    case registration(String, OSStatus)

    var errorDescription: String? {
        switch self {
        case .unsupportedKey(let key):
            return "The key \(key) is unavailable in the current keyboard layout. Record a different shortcut."
        case .missingModifier:
            return "The global shortcut needs Command, Control, or Option."
        case .handlerInstallation(let status):
            return "Time Tracker could not install its shortcut handler. Restart the app and try again. macOS error \(status)."
        case .registration(let shortcut, let status):
            return "macOS could not register \(shortcut). Another app or a system shortcut may use it. Choose a different shortcut. macOS error \(status)."
        }
    }
}

@MainActor
final class GlobalMenuShortcutRegistration {
    private static let signature: OSType = 0x54544D53
    private let action: @MainActor () -> Void
    private var handler: EventHandlerRef?
    private var hotKey: EventHotKeyRef?
    private var shortcut: MenuShortcut?
    private var registeredKeyCode: UInt32?
    private var identifier: UInt32 = 0

    init(action: @escaping @MainActor () -> Void) {
        self.action = action
    }

    deinit {
        if let hotKey { UnregisterEventHotKey(hotKey) }
        if let handler { RemoveEventHandler(handler) }
    }

    func register(_ shortcut: MenuShortcut) throws {
        guard !shortcut.modifiers.intersection([.command, .control, .option]).isEmpty else {
            throw GlobalMenuShortcutError.missingModifier
        }
        guard let keyCode = KeyboardLayoutKeyCode.find(for: shortcut) else {
            throw GlobalMenuShortcutError.unsupportedKey(shortcut.key)
        }
        if self.shortcut == shortcut, registeredKeyCode == keyCode, hotKey != nil { return }
        try installHandler()
        let nextIdentifier = identifier &+ 1
        let hotKeyID = EventHotKeyID(signature: Self.signature, id: nextIdentifier)
        var replacement: EventHotKeyRef?
        let status = RegisterEventHotKey(keyCode, carbonModifiers(shortcut.modifiers), hotKeyID,
                                         GetApplicationEventTarget(), 0, &replacement)
        guard status == noErr, let replacement else {
            throw GlobalMenuShortcutError.registration(shortcut.displayText, status)
        }
        // Keep the previous shortcut registered if macOS rejects its replacement.
        let previous = hotKey
        hotKey = replacement
        self.shortcut = shortcut
        registeredKeyCode = keyCode
        identifier = nextIdentifier
        if let previous { UnregisterEventHotKey(previous) }
    }

    func unregister() {
        if let hotKey { UnregisterEventHotKey(hotKey) }
        hotKey = nil
        shortcut = nil
        registeredKeyCode = nil
        if let handler { RemoveEventHandler(handler) }
        handler = nil
    }

    private func installHandler() throws {
        guard handler == nil else { return }
        var eventType = EventTypeSpec(eventClass: OSType(kEventClassKeyboard),
                                      eventKind: UInt32(kEventHotKeyPressed))
        let status = InstallEventHandler(GetApplicationEventTarget(), { _, event, context in
            guard let event, let context else { return OSStatus(eventNotHandledErr) }
            var hotKeyID = EventHotKeyID()
            let status = GetEventParameter(event, EventParamName(kEventParamDirectObject),
                                           EventParamType(typeEventHotKeyID), nil,
                                           MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID)
            guard status == noErr else { return status }
            return MainActor.assumeIsolated {
                let registration = Unmanaged<GlobalMenuShortcutRegistration>
                    .fromOpaque(context).takeUnretainedValue()
                guard hotKeyID.signature == GlobalMenuShortcutRegistration.signature,
                      hotKeyID.id == registration.identifier,
                      registration.hotKey != nil else { return OSStatus(eventNotHandledErr) }
                if let shortcut = registration.shortcut,
                   let recorder = NSApplication.shared.keyWindow?.firstResponder as? ShortcutRecorderButton,
                   recorder.captureRegisteredShortcut(shortcut) {
                    return noErr
                }
                registration.action()
                return noErr
            }
        }, 1, &eventType, Unmanaged.passUnretained(self).toOpaque(), &handler)
        guard status == noErr else { throw GlobalMenuShortcutError.handlerInstallation(status) }
    }

    private func carbonModifiers(_ modifiers: MenuShortcut.Modifiers) -> UInt32 {
        var result: UInt32 = 0
        if modifiers.contains(.command) { result |= UInt32(cmdKey) }
        if modifiers.contains(.control) { result |= UInt32(controlKey) }
        if modifiers.contains(.option) { result |= UInt32(optionKey) }
        if modifiers.contains(.shift) { result |= UInt32(shiftKey) }
        return result
    }
}

private enum KeyboardLayoutKeyCode {
    static func find(for shortcut: MenuShortcut) -> UInt32? {
        let sources = [TISCopyCurrentKeyboardLayoutInputSource()?.takeRetainedValue(),
                       TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue()]
        for source in sources.compactMap({ $0 }) {
            guard let property = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else {
                continue
            }
            if let keyCode = withExtendedLifetime(source, {
                find(for: shortcut, data: Unmanaged<CFData>.fromOpaque(property).takeUnretainedValue())
            }) { return keyCode }
        }
        return nil
    }

    private static func find(for shortcut: MenuShortcut, data: CFData) -> UInt32? {
        guard let bytes = CFDataGetBytePtr(data) else { return nil }
        let layout = UnsafeRawPointer(bytes).assumingMemoryBound(to: UCKeyboardLayout.self)
        let modifiers = shortcut.modifiers.contains(.shift) ? UInt32(shiftKey >> 8) : 0
        return withExtendedLifetime(data) {
            for keyCode in UInt16(0)..<128 {
                var deadKeyState: UInt32 = 0
                var characters = [UniChar](repeating: 0, count: 8)
                var count = 0
                let status = UCKeyTranslate(layout, keyCode, UInt16(kUCKeyActionDisplay), modifiers,
                                            UInt32(LMGetKbdType()),
                                            OptionBits(1 << kUCKeyTranslateNoDeadKeysBit),
                                            &deadKeyState, characters.count, &count, &characters)
                guard status == noErr, count > 0 else { continue }
                let key = String(utf16CodeUnits: characters, count: count).lowercased()
                if key == shortcut.key { return UInt32(keyCode) }
            }
            return nil
        }
    }
}

@MainActor
struct ShortcutRecorder: NSViewRepresentable {
    var shortcut: MenuShortcut
    var onRecord: (MenuShortcut) -> Void

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

@MainActor
final class ShortcutRecorderButton: NSButton {
    var shortcut: MenuShortcut?
    var onRecord: ((MenuShortcut) -> Void)?
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

    deinit {
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
            if event.modifierFlags.contains(.shift) { window?.selectPreviousKeyView(self) }
            else { window?.selectNextKeyView(self) }
            return true
        }
        guard let shortcut = MenuShortcut(event: event) else {
            NSSound.beep()
            return true
        }
        finishRecording()
        onRecord?(shortcut)
        return true
    }

    func captureRegisteredShortcut(_ shortcut: MenuShortcut) -> Bool {
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
