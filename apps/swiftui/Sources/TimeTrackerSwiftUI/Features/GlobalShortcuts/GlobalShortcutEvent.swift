import AppKit
import Carbon
import TrackerClient

extension GlobalShortcut {
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

extension GlobalShortcut.Modifiers {
    init(_ flags: NSEvent.ModifierFlags) {
        self.init()
        if flags.contains(.command) { insert(.command) }
        if flags.contains(.control) { insert(.control) }
        if flags.contains(.option) { insert(.option) }
        if flags.contains(.shift) { insert(.shift) }
    }
}
