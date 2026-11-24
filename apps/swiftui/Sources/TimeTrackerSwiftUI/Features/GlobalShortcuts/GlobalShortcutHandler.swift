import AppKit
import Carbon
import Combine
import TrackerClient

/// Keeps the two global shortcuts, Open menu and Open Quick Track, registered with macOS. It registers the saved
/// shortcuts, registers them again when the keyboard layout changes, lets the store register a new
/// shortcut, and reports errors there. Creating it registers the shortcuts and replaces the store's
/// `registerShortcut`, so the app creates one.
final class GlobalShortcutHandler {
    private let store: GlobalShortcutsStore
    private let registration: GlobalShortcutRegistration
    private var layoutChanges: AnyCancellable?

    init(
        store: GlobalShortcutsStore, toggleMenu: @escaping @MainActor () -> Void,
        openQuickTrack: @escaping @MainActor () -> Void
    ) {
        self.store = store
        let registration = GlobalShortcutRegistration(toggleMenu: toggleMenu, openQuickTrack: openQuickTrack)
        self.registration = registration
        store.registerShortcut = { [weak registration] action, shortcut in
            do {
                try registration?.register(action, shortcut)
                return nil
            } catch { return error.localizedDescription }
        }
        store.reportRegistrationError(registerSavedShortcuts())
        layoutChanges = DistributedNotificationCenter.default()
            .publisher(for: Notification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String))
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.store.reportRegistrationError(self.registerSavedShortcuts())
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
                store.reportRegistrationError(registerSavedShortcuts())
            }
        } catch {
            // Open menu failed again; keep Open Quick Track's error next to it.
            let errors = [error.localizedDescription, registerSavedShortcuts([.openQuickTrack])].compactMap { $0 }
            store.reportRegistrationError(errors.joined(separator: " "))
        }
    }

    /// Registers the saved global shortcuts, even when one fails, and returns the errors, if any.
    private func registerSavedShortcuts(_ actions: [GlobalShortcutAction] = [.openMenu, .openQuickTrack]) -> String? {
        let errors = actions.compactMap { action -> String? in
            do {
                try registration.register(action, store.shortcuts[action])
                return nil
            } catch { return error.localizedDescription }
        }
        return errors.isEmpty ? nil : errors.joined(separator: " ")
    }
}
