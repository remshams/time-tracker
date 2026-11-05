import Foundation
import Observation

/// Holds the connection settings and status for the whole app.
///
/// Views read the public properties; `TrackerSync` writes them. Each property is assigned only when its value
/// changes, so an unchanged poll redraws no connection reader.
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

    init(settings: ConnectionSettings) { self.settings = settings }

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
