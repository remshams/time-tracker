import AppKit
import Carbon
import Combine
import TrackerClient

/// Keeps the open menu shortcut registered with macOS. It registers the saved shortcut, registers it again when
/// the keyboard layout changes, lets the preferences store register a new shortcut, and reports errors there.
final class GlobalMenuShortcutHandler {
    private let preferences: MenuBarPreferencesStore
    private let registration: GlobalMenuShortcutRegistration
    private var layoutChanges: AnyCancellable?

    init(preferences: MenuBarPreferencesStore, toggleMenu: @escaping @MainActor () -> Void) {
        self.preferences = preferences
        let registration = GlobalMenuShortcutRegistration(action: toggleMenu)
        self.registration = registration
        preferences.registerMenuShortcut = { [weak registration] shortcut in
            do {
                try registration?.register(shortcut)
                return nil
            } catch { return error.localizedDescription }
        }
        do { try registration.register(preferences.menuShortcuts.openMenu) } catch {
            preferences.reportMenuGlobalShortcutError(error.localizedDescription)
        }
        layoutChanges = DistributedNotificationCenter.default()
            .publisher(for: Notification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String))
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                MainActor.assumeIsolated { self?.registerAfterLayoutChange() }
            }
    }

    /// Lets the shortcut close the open menu, which suspends the global registration.
    func beginMenuTracking(dismiss: @escaping @MainActor () -> Void) {
        registration.beginMenuTracking(dismiss: dismiss)
    }

    func endMenuTracking() {
        do {
            if try registration.endMenuTracking() { preferences.reportMenuGlobalShortcutError(nil) }
        } catch { preferences.reportMenuGlobalShortcutError(error.localizedDescription) }
    }

    private func registerAfterLayoutChange() {
        do {
            try registration.register(preferences.menuShortcuts.openMenu)
            preferences.reportMenuGlobalShortcutError(nil)
        } catch { preferences.reportMenuGlobalShortcutError(error.localizedDescription) }
    }
}
