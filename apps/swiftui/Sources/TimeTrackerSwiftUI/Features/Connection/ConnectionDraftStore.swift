import Observation
import SwiftUI
import TrackerClient

/// Holds the unsaved data source shown in Settings and the result of testing or adopting it.
@Observable
final class ConnectionDraftStore {
    private let connection: ConnectionStore
    // Connection tests go straight to the client; they don't touch the stores.
    private let client: any TrackerClient

    var settings: ConnectionSettings
    private(set) var operationPending = false
    private(set) var resultMessage: String?
    private(set) var resultSucceeded = false

    init(connection: ConnectionStore, client: any TrackerClient) {
        self.connection = connection
        self.client = client
        settings = connection.settings
    }

    var missingEndpoint: Bool {
        settings.mode == .server && settings.serverURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Starts a new presentation with the current settings and no result.
    func reset() {
        settings = connection.settings
        operationPending = false
        resultMessage = nil
        resultSucceeded = false
    }

    func clearResult() { resultMessage = nil }

    func testConnection() {
        let settings = self.settings
        operationPending = true
        resultMessage = nil
        Task {
            do {
                try await testConnectionSettings(settings, client: self.client)
                self.resultSucceeded = true
                self.resultMessage = "Connection succeeded. Click Connect to use this data source."
            } catch {
                self.resultSucceeded = false
                self.resultMessage = error.localizedDescription
            }
            self.operationPending = false
        }
    }

    func connect() {
        let settings = self.settings
        operationPending = true
        resultMessage = nil
        Task {
            let succeeded = await self.connection.connect(settings)
            self.resultSucceeded = succeeded
            self.resultMessage =
                self.resultSucceeded
                ? "Connected. This data source will be used next time you open the app."
                : self.connection.message ?? "Could not connect. Please try again."
            self.operationPending = false
        }
    }
}

extension View {
    /// Resets the draft when Settings appears.
    /// Apply it outside the Form so scrolling sections in and out of view does not reset the draft.
    func connectionDraftLifecycle(_ draft: ConnectionDraftStore) -> some View {
        modifier(ConnectionDraftLifecycle(draft: draft))
    }
}

private struct ConnectionDraftLifecycle: ViewModifier {
    let draft: ConnectionDraftStore
    @Environment(ConnectionStore.self) private var connection

    func body(content: Content) -> some View {
        content
            .onAppear { draft.reset() }
            .onChange(of: draft.settings) { _, _ in draft.clearResult() }
            .onChange(of: connection.settings) { _, settings in draft.settings = settings }
    }
}
