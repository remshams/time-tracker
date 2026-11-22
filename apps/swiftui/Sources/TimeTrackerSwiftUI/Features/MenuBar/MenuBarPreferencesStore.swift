import Observation
import TrackerClient

/// Owns the menu bar settings: the daily total and the keyboard shortcuts with their errors.
/// It reads the saved daily total from the menu observer, which the app seeds from the same defaults.
@Observable
final class MenuBarPreferencesStore {
    private let preferences: UserDefaultsMenuBarPreferences
    private let keyboardPreferences: UserDefaultsMenuKeyboardPreferences
    private let menu: TrackerMenuPresentationObserver
    private let menuBar: MenuBarStore
    private(set) var showDailyTotalInMenuBar: Bool
    private(set) var menuShortcuts: MenuKeyboardShortcuts
    private var menuShortcutValidationError: String?
    private(set) var menuGlobalShortcutError: String?
    @ObservationIgnored var registerMenuShortcut: ((MenuShortcutAction, MenuShortcut) -> String?)?
    var menuShortcutError: String? { menuShortcutValidationError ?? menuGlobalShortcutError }

    init(
        preferences: UserDefaultsMenuBarPreferences,
        keyboardPreferences: UserDefaultsMenuKeyboardPreferences,
        menu: TrackerMenuPresentationObserver,
        menuBar: MenuBarStore
    ) {
        self.preferences = preferences
        self.keyboardPreferences = keyboardPreferences
        self.menu = menu
        self.menuBar = menuBar
        showDailyTotalInMenuBar = menu.showDailyTotal
        menuShortcuts = keyboardPreferences.load()
        menuBar.setClockEnabled(menu.showDailyTotal)
    }

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        showDailyTotalInMenuBar = enabled
        preferences.save(enabled)
        menu.setShowDailyTotal(enabled)
        menuBar.setClockEnabled(enabled)
    }

    func setMenuShortcut(_ action: MenuShortcutAction, shortcut: MenuShortcut) {
        // Only the global shortcuts can be edited, and the others stay at their defaults, so a change starts from the
        // defaults and keeps the current global bindings for the duplicate check.
        var candidate = MenuKeyboardShortcuts(
            openMenu: menuShortcuts.openMenu, openQuickTrack: menuShortcuts.openQuickTrack)
        candidate[action] = shortcut
        // Recording a global shortcut again retries a failed global registration.
        setMenuShortcuts(candidate, retryGlobal: menuGlobalShortcutError != nil)
    }

    func resetMenuShortcuts() { setMenuShortcuts(.defaults, retryGlobal: true) }

    private func setMenuShortcuts(_ candidate: MenuKeyboardShortcuts, retryGlobal: Bool) {
        if let error = candidate.validationError {
            reportMenuShortcutError(error)
            return
        }
        let globals = MenuShortcutAction.allCases.filter(\.isGlobal)
        let changed = globals.filter { candidate[$0] != menuShortcuts[$0] }
        var registered: [MenuShortcutAction] = []
        for action in changed {
            if let error = registerMenuShortcut?(action, candidate[action]) {
                // Put back the bindings that were registered before this one failed. If one can't come back, its new
                // binding stays active, so Settings says so.
                let rollbackErrors = registered.compactMap { registerMenuShortcut?($0, menuShortcuts[$0]) }
                if !rollbackErrors.isEmpty { reportMenuGlobalShortcutError(rollbackErrors.joined(separator: " ")) }
                reportMenuShortcutError(error)
                return
            }
            registered.append(action)
        }
        // A retry of an unchanged binding that still fails doesn't block saving the changed one; Settings keeps
        // showing its error.
        let retried = retryGlobal ? globals.filter { !changed.contains($0) } : []
        let retryErrors = retried.compactMap { registerMenuShortcut?($0, candidate[$0]) }
        menuShortcuts = candidate
        reportMenuShortcutError(nil)
        if !changed.isEmpty || !retried.isEmpty {
            reportMenuGlobalShortcutError(retryErrors.isEmpty ? nil : retryErrors.joined(separator: " "))
        }
        keyboardPreferences.save(candidate)
    }

    private func reportMenuShortcutError(_ message: String?) {
        guard menuShortcutValidationError != message else { return }
        menuShortcutValidationError = message
    }

    func reportMenuGlobalShortcutError(_ message: String?) {
        guard menuGlobalShortcutError != message else { return }
        menuGlobalShortcutError = message
    }
}
