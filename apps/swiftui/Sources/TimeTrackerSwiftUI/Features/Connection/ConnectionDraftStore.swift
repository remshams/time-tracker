import Observation
import SwiftUI
import TrackerClient

/// Holds the unsaved data source shown in Settings and the result of testing or adopting it.
@Observable
final class ConnectionDraftStore {
    private let store: TrackerStore

    var settings: ConnectionSettings
    private(set) var operationPending = false
    private(set) var resultMessage: String?
    private(set) var resultSucceeded = false

    init(store: TrackerStore) {
        self.store = store
        settings = store.connectionSettings
    }

    var missingEndpoint: Bool {
        settings.mode == .server && settings.serverURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Starts a new presentation with the current settings and no result.
    func reset() {
        settings = store.connectionSettings
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
                try await self.store.testConnection(settings)
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
            let succeeded = await self.store.connect(settings)
            self.resultSucceeded = succeeded
            self.resultMessage =
                self.resultSucceeded
                ? "Connected. This data source will be used next time you open the app."
                : self.store.connectionMessage ?? "Could not connect. Please try again."
            self.operationPending = false
        }
    }
}

extension View {
    /// Resets the draft when Settings appears.
    /// Apply it outside the Form so scrolling sections in and out of view does not reset the draft.
    func connectionDraftLifecycle(_ draft: ConnectionDraftStore, store: TrackerStore) -> some View {
        modifier(ConnectionDraftLifecycle(store: store, draft: draft))
    }
}

private struct ConnectionDraftLifecycle: ViewModifier {
    let store: TrackerStore
    let draft: ConnectionDraftStore

    func body(content: Content) -> some View {
        content
            .onAppear { draft.reset() }
            .onChange(of: draft.settings) { _, _ in draft.clearResult() }
            .onChange(of: store.connectionSettings) { _, settings in draft.settings = settings }
    }
}
