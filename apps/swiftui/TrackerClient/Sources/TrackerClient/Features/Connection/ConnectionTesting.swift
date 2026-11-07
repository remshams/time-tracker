import Foundation

extension ConnectionSettings {
    /// Returns these settings with the server URL trimmed, or throws when server mode has no URL.
    func normalized() throws -> ConnectionSettings {
        let url = serverURL.trimmingCharacters(in: .whitespacesAndNewlines)
        if mode == .server && url.isEmpty {
            throw BridgeFailure(message: "Enter a server URL.")
        }
        return ConnectionSettings(mode: mode, serverURL: url)
    }
}

/// Checks that `settings` reach a tracker without changing the current connection or saving anything.
///
/// The request goes out right away, even while a poll or another request runs.
public func testConnectionSettings(_ settings: ConnectionSettings, client: any TrackerClient) async throws {
    try await client.test(settings.normalized())
}
