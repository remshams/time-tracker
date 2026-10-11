import Foundation

public enum MenuBarDisplay: String, CaseIterable {
    case time
    case taskName
    case none
}

@MainActor
public final class UserDefaultsMenuBarPreferences {
    private let defaults: UserDefaults
    private let key = "tracker.menuBarDisplay"
    private let legacyKey = "tracker.showDailyTotalInMenuBar"

    public init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    public func load() -> MenuBarDisplay {
        if let rawValue = defaults.string(forKey: key), let display = MenuBarDisplay(rawValue: rawValue) {
            return display
        }
        let showTime = defaults.object(forKey: legacyKey) as? Bool ?? true
        return showTime ? .time : .none
    }

    public func save(_ display: MenuBarDisplay) { defaults.set(display.rawValue, forKey: key) }
}
