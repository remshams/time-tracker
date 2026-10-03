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
