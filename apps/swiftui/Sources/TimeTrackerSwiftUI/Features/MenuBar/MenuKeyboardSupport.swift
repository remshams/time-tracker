import AppKit
import Carbon
import SwiftUI
import TrackerClient

extension MenuShortcut {
    init?(event: NSEvent) {
        guard event.type == .keyDown,
            let key = Self.recordableKey(for: event),
            key.unicodeScalars.count == 1,
            let scalar = key.unicodeScalars.first,
            (33...126).contains(scalar.value) || scalar == " "
        else { return nil }
        self.init(key: key, modifiers: Modifiers(event.modifierFlags))
    }

    /// Space is read from its key code: with Control held, AppKit reports it as a control character.
    private static func recordableKey(for event: NSEvent) -> String? {
        if event.keyCode == UInt16(kVK_Space) { return " " }
        return event.characters(byApplyingModifiers: event.modifierFlags.intersection(.shift))
    }
}

private extension MenuShortcut.Modifiers {
    init(_ flags: NSEvent.ModifierFlags) {
        self.init()
        if flags.contains(.command) { insert(.command) }
        if flags.contains(.control) { insert(.control) }
        if flags.contains(.option) { insert(.option) }
        if flags.contains(.shift) { insert(.shift) }
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
            return
                "Time Tracker could not install its shortcut handler. Restart the app and try again. macOS error \(status)."
        case .registration(let shortcut, let status):
            return
                "macOS could not register \(shortcut). Another app or a system shortcut may use it. Choose a different shortcut. macOS error \(status)."
        }
    }
}

final class GlobalMenuShortcutRegistration {
    private static let signature: OSType = 0x54544D53
    /// One Carbon hotkey. Each registration gets a new identifier, so late events from a replaced hotkey are ignored.
    private struct Slot {
        var shortcut: MenuShortcut?
        var keyCode: UInt32?
        var hotKey: EventHotKeyRef?
        var identifier: UInt32 = 0
    }

    private let actions: [MenuShortcutAction: @MainActor () -> Void]
    private var slots: [MenuShortcutAction: Slot] = [.openMenu: Slot(), .openQuickTrack: Slot()]
    private var lastIdentifier: UInt32 = 0
    private var handler: EventHandlerRef?
    private var menuEventMonitor: Any?
    private var menuRunLoopObserver: CFRunLoopObserver?
    private var dismissTrackedMenu: (@MainActor () -> Void)?
    private var isConsumingTrackedShortcut = false

    /// `toggleMenu` and `openQuickTrack` are the actions of the two global shortcuts.
    init(toggleMenu: @escaping @MainActor () -> Void, openQuickTrack: @escaping @MainActor () -> Void) {
        actions = [.openMenu: toggleMenu, .openQuickTrack: openQuickTrack]
    }

    private var menuSlot: Slot { slots[.openMenu] ?? Slot() }

    isolated deinit {
        if let menuEventMonitor { NSEvent.removeMonitor(menuEventMonitor) }
        if let menuRunLoopObserver {
            CFRunLoopRemoveObserver(
                CFRunLoopGetMain(), menuRunLoopObserver,
                CFRunLoopMode(rawValue: RunLoop.Mode.eventTracking.rawValue as CFString))
        }
        for slot in slots.values { if let hotKey = slot.hotKey { UnregisterEventHotKey(hotKey) } }
        if let handler { RemoveEventHandler(handler) }
    }

    /// Registers one global shortcut. The other global shortcut stays registered.
    func register(_ action: MenuShortcutAction, _ shortcut: MenuShortcut) throws {
        precondition(action.isGlobal, "\(action) is not a global shortcut")
        let slot = slots[action] ?? Slot()
        guard !shortcut.modifiers.intersection([.command, .control, .option]).isEmpty else {
            throw GlobalMenuShortcutError.missingModifier
        }
        guard let keyCode = shortcut.isSpace ? UInt32(kVK_Space) : KeyboardLayoutKeyCode.find(for: shortcut)
        else {
            throw GlobalMenuShortcutError.unsupportedKey(shortcut.key)
        }
        if slot.shortcut == shortcut, slot.keyCode == keyCode, slot.hotKey != nil { return }
        try installHandler()
        let nextIdentifier = lastIdentifier &+ 1
        let hotKeyID = EventHotKeyID(signature: Self.signature, id: nextIdentifier)
        var replacement: EventHotKeyRef?
        #if DEBUG
            var testReservation: EventHotKeyRef?
            if TrackerLaunchConfiguration.uiTestSuite != nil,
                ProcessInfo.processInfo.environment["TT_UI_TEST_CONFLICT_KEY"] == shortcut.key
            {
                RegisterEventHotKey(
                    keyCode, carbonModifiers(shortcut.modifiers),
                    EventHotKeyID(signature: 0x54545549, id: 1),
                    GetEventDispatcherTarget(), 0, &testReservation)
            }
            defer { if let testReservation { UnregisterEventHotKey(testReservation) } }
        #endif
        let status = RegisterEventHotKey(
            keyCode, carbonModifiers(shortcut.modifiers), hotKeyID,
            GetEventDispatcherTarget(), 0, &replacement)
        guard status == noErr, let replacement else {
            throw GlobalMenuShortcutError.registration(shortcut.displayText, status)
        }
        // Keep the previous shortcut registered if macOS rejects its replacement.
        let previous = slot.hotKey
        lastIdentifier = nextIdentifier
        slots[action] = Slot(
            shortcut: shortcut, keyCode: keyCode, hotKey: replacement, identifier: nextIdentifier)
        if let previous { UnregisterEventHotKey(previous) }
        if action == .openMenu, dismissTrackedMenu != nil {
            UnregisterEventHotKey(replacement)
            slots[action]?.hotKey = nil
        }
    }

    func beginMenuTracking(dismiss: @escaping @MainActor () -> Void) {
        guard dismissTrackedMenu == nil, menuSlot.shortcut != nil, menuSlot.keyCode != nil else { return }
        // Menu tracking consumes keys before AppKit dispatch and suspends Carbon hotkey delivery.
        // Only the Open menu shortcut matters while the menu is open. Quick Track does nothing then.
        if let hotKey = menuSlot.hotKey { UnregisterEventHotKey(hotKey) }
        slots[.openMenu]?.hotKey = nil
        dismissTrackedMenu = dismiss
        menuEventMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            let consumed = MainActor.assumeIsolated {
                guard let self, self.matchesTrackedShortcut(event) else { return false }
                self.dismissTrackedMenu?()
                return true
            }
            return consumed ? nil : event
        }
        let observer = CFRunLoopObserverCreateWithHandler(
            nil, CFRunLoopActivity.beforeSources.rawValue, true, 0
        ) { [weak self] _, _ in
            MainActor.assumeIsolated { self?.consumeTrackedShortcut() }
        }
        if let observer {
            menuRunLoopObserver = observer
            CFRunLoopAddObserver(
                CFRunLoopGetMain(), observer,
                CFRunLoopMode(rawValue: RunLoop.Mode.eventTracking.rawValue as CFString))
        }
    }

    func endMenuTracking() throws -> Bool {
        let wasTracking = dismissTrackedMenu != nil
        stopMenuTracking()
        guard wasTracking, let shortcut = menuSlot.shortcut else { return false }
        try register(.openMenu, shortcut)
        return true
    }

    private func stopMenuTracking() {
        if let menuEventMonitor { NSEvent.removeMonitor(menuEventMonitor) }
        menuEventMonitor = nil
        if let menuRunLoopObserver {
            CFRunLoopRemoveObserver(
                CFRunLoopGetMain(), menuRunLoopObserver,
                CFRunLoopMode(rawValue: RunLoop.Mode.eventTracking.rawValue as CFString))
        }
        menuRunLoopObserver = nil
        dismissTrackedMenu = nil
    }

    private func matchesTrackedShortcut(_ event: NSEvent) -> Bool {
        guard dismissTrackedMenu != nil, let shortcut = menuSlot.shortcut, let registeredKeyCode = menuSlot.keyCode,
            event.type == .keyDown, !event.isARepeat,
            UInt32(event.keyCode) == registeredKeyCode
        else { return false }
        return MenuShortcut.Modifiers(event.modifierFlags) == shortcut.modifiers
    }

    private func consumeTrackedShortcut() {
        guard !isConsumingTrackedShortcut else { return }
        isConsumingTrackedShortcut = true
        defer { isConsumingTrackedShortcut = false }
        let application = NSApplication.shared
        // Leave other keys and pointer events in their original queue order for the menu.
        guard
            let head = application.nextEvent(
                matching: .any, until: .distantPast,
                inMode: .eventTracking, dequeue: false),
            matchesTrackedShortcut(head),
            application.nextEvent(
                matching: .keyDown, until: .distantPast,
                inMode: .eventTracking, dequeue: true) != nil
        else { return }
        dismissTrackedMenu?()
    }

    private func installHandler() throws {
        guard handler == nil else { return }
        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed))
        let status = InstallEventHandler(
            GetEventDispatcherTarget(),
            { _, event, context in
                guard let event, let context else { return OSStatus(eventNotHandledErr) }
                var hotKeyID = EventHotKeyID()
                let status = GetEventParameter(
                    event, EventParamName(kEventParamDirectObject),
                    EventParamType(typeEventHotKeyID), nil,
                    MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID)
                guard status == noErr else { return status }
                return MainActor.assumeIsolated {
                    let registration = Unmanaged<GlobalMenuShortcutRegistration>
                        .fromOpaque(context).takeUnretainedValue()
                    guard hotKeyID.signature == GlobalMenuShortcutRegistration.signature,
                        let action = registration.registeredAction(identifier: hotKeyID.id)
                    else { return OSStatus(eventNotHandledErr) }
                    if let shortcut = registration.slots[action]?.shortcut,
                        let recorder = NSApplication.shared.keyWindow?.firstResponder as? ShortcutRecorderButton,
                        recorder.captureRegisteredShortcut(shortcut)
                    {
                        return noErr
                    }
                    let triggeredID = hotKeyID.id
                    // Menu tracking starts a nested run loop. Return from Carbon before opening it.
                    RunLoop.main.perform(inModes: [.common]) { [weak registration] in
                        MainActor.assumeIsolated {
                            guard let registration,
                                registration.registeredAction(identifier: triggeredID) == action,
                                // Quick Track does nothing while the status menu is open.
                                action == .openMenu || registration.dismissTrackedMenu == nil
                            else { return }
                            registration.actions[action]?()
                        }
                    }
                    return noErr
                }
            }, 1, &eventType, Unmanaged.passUnretained(self).toOpaque(), &handler)
        guard status == noErr else { throw GlobalMenuShortcutError.handlerInstallation(status) }
    }

    private func registeredAction(identifier: UInt32) -> MenuShortcutAction? {
        slots.first { $0.value.identifier == identifier && $0.value.hotKey != nil }?.key
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
        let sources = [
            TISCopyCurrentKeyboardLayoutInputSource()?.takeRetainedValue(),
            TISCopyCurrentASCIICapableKeyboardLayoutInputSource()?.takeRetainedValue(),
        ]
        for source in sources.compactMap({ $0 }) {
            guard let property = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) else {
                continue
            }
            if let keyCode = withExtendedLifetime(
                source,
                {
                    find(for: shortcut, data: Unmanaged<CFData>.fromOpaque(property).takeUnretainedValue())
                })
            {
                return keyCode
            }
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
                let status = UCKeyTranslate(
                    layout, keyCode, UInt16(kUCKeyActionDisplay), modifiers,
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
