import Foundation
import TrackerClient

@MainActor
struct SystemTrackerClock: TrackerClock {
    var now: Date { Date() }
    var uptime: TimeInterval { ProcessInfo.processInfo.systemUptime }
}

@MainActor
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

@MainActor
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

@MainActor
final class UserDefaultsMenuBarPreferences {
    private let defaults: UserDefaults
    private let key = "tracker.showDailyTotalInMenuBar"

    init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    func load() -> Bool { defaults.object(forKey: key) as? Bool ?? true }

    func save(_ showDailyTotal: Bool) { defaults.set(showDailyTotal, forKey: key) }
}

@MainActor
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

@MainActor
struct RunLoopTrackerScheduler: TrackerScheduler {
    func schedule(after interval: TimeInterval, repeating: Bool, tolerance: TimeInterval,
                  action: @escaping @MainActor () -> Void) -> any TrackerCancellation {
        let timer = Timer(timeInterval: interval, repeats: repeating) { _ in
            Task { @MainActor in action() }
        }
        timer.tolerance = tolerance
        RunLoop.main.add(timer, forMode: .common)
        return TimerCancellation(timer: timer)
    }
}

private final class TimerCancellation: TrackerCancellation {
    private var timer: Timer?
    init(timer: Timer) { self.timer = timer }
    func cancel() { timer?.invalidate(); timer = nil }
    deinit { timer?.invalidate() }
}
