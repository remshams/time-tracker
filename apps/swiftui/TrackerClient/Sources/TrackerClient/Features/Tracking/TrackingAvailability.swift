import Foundation

/// Decides which tracking requests are possible right now.
///
/// It keeps no state. Each check reads the stores, so `Observations` and SwiftUI track what a caller's check read.
public struct TrackingAvailability: Sendable {
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let tracking: TrackingStore

    init(connection: ConnectionStore, catalog: TaskCatalogStore, tracking: TrackingStore) {
        self.connection = connection
        self.catalog = catalog
        self.tracking = tracking
    }

    /// Whether the task exists, isn't archived and isn't already running, and a request can be sent.
    public func canStart(taskID: String) -> Bool {
        guard canSend, let task = catalog.tasks.first(where: { $0.id == taskID }) else { return false }
        return !task.archived && tracking.active?.taskId != task.id
    }

    public var canStop: Bool { canSend && tracking.active != nil }

    /// Whether the tracker state is confirmed and current, and no tracking request is in flight.
    var canSend: Bool { connection.confirmed && !connection.stale && !tracking.isSubmitting }
}
