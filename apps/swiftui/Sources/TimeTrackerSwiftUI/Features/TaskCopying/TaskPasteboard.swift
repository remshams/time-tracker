import AppKit

/// Writes a copied task value to the general pasteboard, or beeps when that fails.
enum TaskPasteboard {
    static func copy(_ value: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        if !pasteboard.setString(value, forType: .string) { NSSound.beep() }
    }
}
