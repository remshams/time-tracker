import Foundation

@MainActor
final class ConnectionState {
    var settings: ConnectionSettings
    var statusText = "Connecting"
    var message: String?
    var changing = false
    var stale = true
    var confirmed = false
    var failures = 0
    var protocolBlocked = false

    init(settings: ConnectionSettings) { self.settings = settings }

    func normalized(_ settings: ConnectionSettings) throws -> ConnectionSettings {
        let url = settings.serverURL.trimmingCharacters(in: .whitespacesAndNewlines)
        if settings.mode == .server && url.isEmpty {
            throw BridgeFailure(message: "Enter a server URL.")
        }
        return ConnectionSettings(mode: settings.mode, serverURL: url)
    }

    func acceptSnapshot() {
        confirmed = true
        stale = false
        failures = 0
        protocolBlocked = false
        statusText = settings.mode == .local ? "Local database" : "Connected"
        message = nil
    }

    func recordFailure(_ failure: Error) {
        stale = true
        failures += 1
        protocolBlocked = (failure as? BridgeFailure)?.kind == "protocol"
        statusText = protocolBlocked ? "Incompatible server" : "Unavailable"
        message = failure.localizedDescription
    }
}

public enum TrackerPollingPolicy {
    public static func interval(visible: Bool, failures: Int) -> TimeInterval {
        let base: TimeInterval = visible ? 5 : 60
        let backoff = 5 * pow(2, Double(min(max(failures - 1, 0), 4)))
        return max(base, min(60, backoff))
    }
}
