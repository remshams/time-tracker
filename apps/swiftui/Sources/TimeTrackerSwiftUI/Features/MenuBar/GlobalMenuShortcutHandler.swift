import AppKit
import Carbon
import Combine
import TrackerClient

/// Keeps the two global shortcuts, Open menu and Open Quick Track, registered with macOS. It registers the saved
/// shortcuts, registers them again when the keyboard layout changes, lets the preferences store register a new
/// shortcut, and reports errors there. Creating it registers the shortcuts and replaces the store's
/// `registerMenuShortcut`, so the app creates one.
final class GlobalMenuShortcutHandler {
    private let preferences: MenuBarPreferencesStore
    private let registration: GlobalMenuShortcutRegistration
    private var layoutChanges: AnyCancellable?

    init(
        preferences: MenuBarPreferencesStore, toggleMenu: @escaping @MainActor () -> Void,
        openQuickTrack: @escaping @MainActor () -> Void
    ) {
        self.preferences = preferences
        let registration = GlobalMenuShortcutRegistration(toggleMenu: toggleMenu, openQuickTrack: openQuickTrack)
        self.registration = registration
        preferences.registerMenuShortcut = { [weak registration] action, shortcut in
            do {
                try registration?.register(action, shortcut)
                return nil
            } catch { return error.localizedDescription }
        }
        preferences.reportMenuGlobalShortcutError(registerSavedShortcuts())
        layoutChanges = DistributedNotificationCenter.default()
            .publisher(for: Notification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String))
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.preferences.reportMenuGlobalShortcutError(self.registerSavedShortcuts())
                }
            }
    }

    /// Lets the shortcut close the open menu, which suspends the global registration.
    func beginMenuTracking(dismiss: @escaping @MainActor () -> Void) {
        registration.beginMenuTracking(dismiss: dismiss)
    }

    func endMenuTracking() {
        do {
            // The error can belong to Open Quick Track, so a working Open menu hotkey doesn't clear it. Registering both
            // again reports what still fails.
            if try registration.endMenuTracking() {
                preferences.reportMenuGlobalShortcutError(registerSavedShortcuts())
            }
        } catch { preferences.reportMenuGlobalShortcutError(error.localizedDescription) }
    }

    /// Registers both global shortcuts, even when the first fails, and returns the errors, if any.
    private func registerSavedShortcuts() -> String? {
        let errors = [MenuShortcutAction.openMenu, .openQuickTrack].compactMap { action -> String? in
            do {
                try registration.register(action, preferences.menuShortcuts[action])
                return nil
            } catch { return error.localizedDescription }
        }
        return errors.isEmpty ? nil : errors.joined(separator: " ")
    }
}
