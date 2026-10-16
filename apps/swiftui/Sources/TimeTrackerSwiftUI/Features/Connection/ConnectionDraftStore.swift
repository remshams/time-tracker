import Combine
import SwiftUI
import TrackerClient

/// Holds the unsaved data source shown in Settings and the result of testing or adopting it.
@MainActor
final class ConnectionDraftStore {
    let objectWillChange = ObservableObjectPublisher()
    private let store: TrackerStore
    private var presentationGeneration = UUID()

    var settings: ConnectionSettings { willSet { objectWillChange.send() } }
    private(set) var operationPending = false { willSet { objectWillChange.send() } }
    private(set) var resultMessage: String? { willSet { objectWillChange.send() } }
    private(set) var resultSucceeded = false { willSet { objectWillChange.send() } }

    init(store: TrackerStore) {
        self.store = store
        settings = store.connectionSettings
    }

    var missingEndpoint: Bool {
        settings.mode == .server && settings.serverURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Starts a new presentation with the current settings and no result.
    func reset() {
        presentationGeneration = UUID()
        settings = store.connectionSettings
        operationPending = false
        resultMessage = nil
        resultSucceeded = false
    }

    /// Ignores results from operations that started before this call.
    func invalidatePendingResults() { presentationGeneration = UUID() }

    func clearResult() { resultMessage = nil }

    func testConnection() {
        let settings = self.settings
        let generation = presentationGeneration
        operationPending = true
        resultMessage = nil
        Task { @MainActor in
            do {
                try await self.store.testConnection(settings)
                guard self.presentationGeneration == generation else { return }
                self.resultSucceeded = true
                self.resultMessage = "Connection succeeded. Click Connect to use this data source."
            } catch {
                guard self.presentationGeneration == generation else { return }
                self.resultSucceeded = false
                self.resultMessage = error.localizedDescription
            }
            self.operationPending = false
        }
    }

    func connect() {
        let settings = self.settings
        let generation = presentationGeneration
        operationPending = true
        resultMessage = nil
        Task { @MainActor in
            let succeeded = await self.store.connect(settings)
            guard self.presentationGeneration == generation else { return }
            self.resultSucceeded = succeeded
            self.resultMessage =
                self.resultSucceeded
                ? "Connected. This data source will be used next time you open the app."
                : self.store.connectionMessage ?? "Could not connect. Please try again."
            self.operationPending = false
        }
    }
}

// Keep publisher access on the main actor with compilers that support isolated conformances.
#if compiler(>=6.2)
    extension ConnectionDraftStore: @MainActor ObservableObject {}
#else
    extension ConnectionDraftStore: ObservableObject {}
#endif

extension View {
    /// Resets the draft when Settings appears and ignores in-flight results after it disappears.
    /// Apply it outside the Form so scrolling sections in and out of view does not reset the draft.
    @MainActor
    func connectionDraftLifecycle(_ draft: ConnectionDraftStore, store: TrackerStore) -> some View {
        modifier(ConnectionDraftLifecycle(store: store, draft: draft))
    }
}

@MainActor
private struct ConnectionDraftLifecycle: ViewModifier {
    @ObservedObject var store: TrackerStore
    @ObservedObject var draft: ConnectionDraftStore

    func body(content: Content) -> some View {
        content
            .onAppear { draft.reset() }
            .onDisappear { draft.invalidatePendingResults() }
            .onChange(of: draft.settings) { _ in draft.clearResult() }
            .onChange(of: store.connectionSettings) { settings in draft.settings = settings }
    }
}
