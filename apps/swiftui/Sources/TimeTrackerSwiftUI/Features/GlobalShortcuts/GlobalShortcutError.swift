import Foundation

enum GlobalShortcutError: LocalizedError {
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
