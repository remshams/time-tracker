import Observation
import TrackerClient

/// Owns the menu bar settings: whether the menu bar shows today's total.
/// It reads the saved daily total from the menu observer, which the app seeds from the same defaults.
@Observable
final class MenuBarPreferencesStore {
    private let preferences: UserDefaultsMenuBarPreferences
    private let menu: TrackerMenuPresentationObserver
    private let menuBar: MenuBarStore
    private(set) var showDailyTotalInMenuBar: Bool

    init(
        preferences: UserDefaultsMenuBarPreferences,
        menu: TrackerMenuPresentationObserver,
        menuBar: MenuBarStore
    ) {
        self.preferences = preferences
        self.menu = menu
        self.menuBar = menuBar
        showDailyTotalInMenuBar = menu.showDailyTotal
        menuBar.setClockEnabled(menu.showDailyTotal)
    }

    func setShowDailyTotalInMenuBar(_ enabled: Bool) {
        guard showDailyTotalInMenuBar != enabled else { return }
        showDailyTotalInMenuBar = enabled
        preferences.save(enabled)
        menu.setShowDailyTotal(enabled)
        menuBar.setClockEnabled(enabled)
    }
}
