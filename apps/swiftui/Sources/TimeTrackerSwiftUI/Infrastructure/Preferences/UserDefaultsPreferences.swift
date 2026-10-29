import Foundation
import TrackerClient

final class UserDefaultsConnectionSettings: ConnectionSettingsRepository {
    private let defaults: UserDefaults
    private let key = "tracker.connection"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load() -> ConnectionSettings? {
        defaults.data(forKey: key).flatMap {
            try? JSONDecoder().decode(ConnectionSettings.self, from: $0)
        }
    }

    func save(_ settings: ConnectionSettings) {
        if let data = try? JSONEncoder().encode(settings) { defaults.set(data, forKey: key) }
    }
}

final class UserDefaultsTrackingPreferences: TrackingPreferencesRepository {
    private let defaults: UserDefaults
    private let key = "tracker.pauseOnScreenLock"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load() -> TrackingPreferences {
        TrackingPreferences(pauseOnScreenLock: defaults.bool(forKey: key))
    }

    func save(_ preferences: TrackingPreferences) {
        defaults.set(preferences.pauseOnScreenLock, forKey: key)
    }
}

final class UserDefaultsMenuBarPreferences {
    private let defaults: UserDefaults
    private let key = "tracker.showDailyTotalInMenuBar"
    private let taskOrderKey = "tracker.menuTaskOrder"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load() -> Bool { defaults.object(forKey: key) as? Bool ?? true }

    func save(_ showDailyTotal: Bool) { defaults.set(showDailyTotal, forKey: key) }

    func loadTaskOrder() -> MenuTaskOrder {
        defaults.string(forKey: taskOrderKey).flatMap(MenuTaskOrder.init(rawValue:)) ?? .newestFirst
    }

    func saveTaskOrder(_ order: MenuTaskOrder) { defaults.set(order.rawValue, forKey: taskOrderKey) }
}

final class UserDefaultsLastTrackedTasks: LastTrackedTaskRepository {
    private let defaults: UserDefaults
    private let key = "tracker.lastTrackedTasks"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load(for settings: ConnectionSettings) -> String? {
        defaults.dictionary(forKey: key)?[settings.trackingIdentityKey] as? String
    }

    func save(taskID: String, for settings: ConnectionSettings) {
        var tasks = defaults.dictionary(forKey: key) ?? [:]
        tasks[settings.trackingIdentityKey] = taskID
        defaults.set(tasks, forKey: key)
    }
}

final class UserDefaultsMenuKeyboardPreferences {
    private let defaults: UserDefaults
    private let key = "tracker.menuKeyboardShortcuts"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load() -> MenuKeyboardShortcuts {
        guard let data = defaults.data(forKey: key),
            let shortcuts = try? JSONDecoder().decode(MenuKeyboardShortcuts.self, from: data)
        else { return .defaults }
        return shortcuts
    }

    func save(_ shortcuts: MenuKeyboardShortcuts) {
        if let data = try? JSONEncoder().encode(shortcuts) { defaults.set(data, forKey: key) }
    }
}
