import Observation
import TrackerClient

/// Owns the menu bar settings: the daily total, the task order and the keyboard shortcuts with their errors.
/// It reads the saved order and daily total from the menu observer, which the app seeds from the same defaults.
@Observable
final class MenuBarPreferencesStore {
    private let preferences: UserDefaultsMenuBarPreferences
    private let keyboardPreferences: UserDefaultsMenuKeyboardPreferences
    private let menu: TrackerMenuPresentationObserver
    private let sync: TrackerSync
    private(set) var showDailyTotalInMenuBar: Bool
    private(set) var menuTaskOrder: MenuTaskOrder
    private(set) var menuShortcuts: MenuKeyboardShortcuts
    private var menuShortcutValidationError: String?
    private(set) var menuGlobalShortcutError: String?
    @ObservationIgnored var registerMenuShortcut: ((MenuShortcut) -> String?)?
    var menuShortcutError: String? { menuShortcutValidationError ?? menuGlobalShortcutError }

    init(
        preferences: UserDefaultsMenuBarPreferences,
        keyboardPreferences: UserDefaultsMenuKeyboardPreferences,
        menu: TrackerMenuPresentationObserver,
        sync: TrackerSync
    ) {
        self.preferences = preferences
        self.keyboardPreferences = keyboardPreferences
        self.menu = menu
        self.sync = sync
        showDailyTotalInMenuBar = menu.showDailyTotal
        menuTaskOrder = menu.taskOrder
        menuShortcuts = keyboardPreferences.load()
        sync.setMenuBarClockEnabled(menu.showDailyTotal)
    }

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        showDailyTotalInMenuBar = enabled
        preferences.save(enabled)
        menu.setShowDailyTotal(enabled)
        sync.setMenuBarClockEnabled(enabled)
    }

    func setMenuTaskOrder(_ order: MenuTaskOrder) {
        guard menuTaskOrder != order else { return }
        menuTaskOrder = order
        preferences.saveTaskOrder(order)
        menu.setTaskOrder(order)
    }

    func setMenuShortcut(_ action: MenuShortcutAction, shortcut: MenuShortcut) {
        var candidate = action == .openMenu ? MenuKeyboardShortcuts() : menuShortcuts
        candidate[action] = shortcut
        // Recording the open menu shortcut again retries a failed global registration.
        setMenuShortcuts(candidate, retryGlobal: action == .openMenu && menuGlobalShortcutError != nil)
    }

    func resetMenuShortcuts() { setMenuShortcuts(.defaults, retryGlobal: true) }

    private func setMenuShortcuts(_ candidate: MenuKeyboardShortcuts, retryGlobal: Bool) {
        if let error = candidate.validationError {
            reportMenuShortcutError(error)
            return
        }
        let changesGlobal = retryGlobal || candidate.openMenu != menuShortcuts.openMenu
        if changesGlobal, let error = registerMenuShortcut?(candidate.openMenu) {
            reportMenuShortcutError(error)
            return
        }
        menuShortcuts = candidate
        reportMenuShortcutError(nil)
        if changesGlobal { reportMenuGlobalShortcutError(nil) }
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
