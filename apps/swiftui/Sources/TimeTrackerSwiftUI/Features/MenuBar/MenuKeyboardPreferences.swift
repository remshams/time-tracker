import Foundation
import TrackerClient

@MainActor
final class UserDefaultsMenuKeyboardPreferences {
    private let defaults: UserDefaults
    private let key = "tracker.menuKeyboardShortcuts"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load() -> MenuKeyboardShortcuts {
        guard let data = defaults.data(forKey: key),
              let shortcuts = try? JSONDecoder().decode(MenuKeyboardShortcuts.self, from: data),
              shortcuts.validationError == nil else { return .defaults }
        return shortcuts
    }

    func save(_ shortcuts: MenuKeyboardShortcuts) {
        guard shortcuts.validationError == nil,
              let data = try? JSONEncoder().encode(shortcuts) else { return }
        defaults.set(data, forKey: key)
    }
}
