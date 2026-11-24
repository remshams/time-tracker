import AppKit
import Carbon
import TrackerClient

final class GlobalShortcutRegistration {
    private static let signature: OSType = 0x54544D53
    /// One Carbon hotkey. Each registration gets a new identifier, so late events from a replaced hotkey are ignored.
    private struct Slot {
        var shortcut: GlobalShortcut?
        var keyCode: UInt32?
        var hotKey: EventHotKeyRef?
        var identifier: UInt32 = 0
    }

    private let actions: [GlobalShortcutAction: @MainActor () -> Void]
    private var slots: [GlobalShortcutAction: Slot] = [.openMenu: Slot(), .openQuickTrack: Slot()]
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
    func register(_ action: GlobalShortcutAction, _ shortcut: GlobalShortcut) throws {
        let slot = slots[action] ?? Slot()
        guard !shortcut.modifiers.intersection([.command, .control, .option]).isEmpty else {
            throw GlobalShortcutError.missingModifier
        }
        guard let keyCode = shortcut.isSpace ? UInt32(kVK_Space) : KeyboardLayoutKeyCode.find(for: shortcut)
        else {
            throw GlobalShortcutError.unsupportedKey(shortcut.key)
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
            throw GlobalShortcutError.registration(shortcut.displayText, status)
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
        return GlobalShortcut.Modifiers(event.modifierFlags) == shortcut.modifiers
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
                    let registration = Unmanaged<GlobalShortcutRegistration>
                        .fromOpaque(context).takeUnretainedValue()
                    guard hotKeyID.signature == GlobalShortcutRegistration.signature,
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
        guard status == noErr else { throw GlobalShortcutError.handlerInstallation(status) }
    }

    private func registeredAction(identifier: UInt32) -> GlobalShortcutAction? {
        slots.first { $0.value.identifier == identifier && $0.value.hotKey != nil }?.key
    }

    private func carbonModifiers(_ modifiers: GlobalShortcut.Modifiers) -> UInt32 {
        var result: UInt32 = 0
        if modifiers.contains(.command) { result |= UInt32(cmdKey) }
        if modifiers.contains(.control) { result |= UInt32(controlKey) }
        if modifiers.contains(.option) { result |= UInt32(optionKey) }
        if modifiers.contains(.shift) { result |= UInt32(shiftKey) }
        return result
    }
}

private enum KeyboardLayoutKeyCode {
    static func find(for shortcut: GlobalShortcut) -> UInt32? {
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

    private static func find(for shortcut: GlobalShortcut, data: CFData) -> UInt32? {
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
