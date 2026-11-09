import Foundation
import Observation

/// Holds the connection settings and status for the whole app.
///
/// Views read the public properties, connect to other settings and retry a failed connection; `TrackerSync` writes the
/// properties too. Each property is assigned only when its value changes, so an unchanged poll redraws no connection
/// reader.
@Observable
public final class ConnectionStore: ChangeGuardedStore {
    public private(set) var settings: ConnectionSettings
    public private(set) var statusText = "Connecting"
    public private(set) var message: String?
    public private(set) var changing = false
    public private(set) var stale = true
    public private(set) var confirmed = false
    // Only polling and refresh read these.
    @ObservationIgnored private(set) var failures = 0
    @ObservationIgnored private(set) var protocolBlocked = false
    /// Opens the given settings, saves them and resets and reloads the stores, so that `TrackerSync` can switch the
    /// connection. Throws when the tracker rejects the settings. `TrackerFeatures` sets it once, after it creates
    /// `TrackerSync`, which needs this store.
    @ObservationIgnored var switchConnection: @MainActor (ConnectionSettings) async throws -> Void = { _ in }
    /// Reloads the task list now. `TrackerFeatures` sets it to `TrackerSync`'s refresh.
    @ObservationIgnored var refreshTaskList: @MainActor () -> Void = {}

    init(settings: ConnectionSettings) { self.settings = settings }

    /// Switches to `settings`, saves them and loads their task list and today's totals. Returns false when
    /// another change is running or the tracker rejects the settings; the current connection then stays.
    public func connect(_ settings: ConnectionSettings) async -> Bool {
        guard !changing else { return false }
        beginChange()
        defer { endChange() }
        do {
            try await switchConnection(try settings.normalized())
        } catch {
            rejectChange(error)
            return false
        }
        return true
    }

    /// Reloads the task list now, for example after a failed connection. Does nothing before the first load starts,
    /// while the Mac sleeps or while a refresh runs.
    public func refresh() { refreshTaskList() }

    func beginChange() { set(\.changing, true) }

    func endChange() { set(\.changing, false) }

    func rejectChange(_ failure: Error) {
        set(\.message, "Connection unchanged. \(failure.localizedDescription)")
    }

    func apply(_ settings: ConnectionSettings) { set(\.settings, settings) }

    func acceptTaskList() {
        set(\.confirmed, true)
        set(\.stale, false)
        failures = 0
        protocolBlocked = false
        set(\.statusText, settings.mode == .local ? "Local database" : "Connected")
        set(\.message, nil)
    }

    func recordFailure(_ failure: Error) {
        set(\.stale, true)
        failures += 1
        protocolBlocked = (failure as? BridgeFailure)?.kind == "protocol"
        set(\.statusText, protocolBlocked ? "Incompatible server" : "Unavailable")
        set(\.message, failure.localizedDescription)
    }
}

public enum TrackerPollingPolicy {
    public static func interval(visible: Bool, failures: Int) -> TimeInterval {
        let base: TimeInterval = visible ? 5 : 60
        let backoff = 5 * pow(2, Double(min(max(failures - 1, 0), 4)))
        return max(base, min(60, backoff))
    }
}
