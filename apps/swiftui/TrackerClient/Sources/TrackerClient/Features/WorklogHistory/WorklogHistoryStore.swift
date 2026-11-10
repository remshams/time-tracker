import Foundation
import Observation

/// Holds the selected task's worklog pages and the history panel's loading and error state.
///
/// Views read the properties, retry the selected task's first page and load older pages. The task catalog's selection
/// changes ask the store to reload, and `acceptTaskList` decides whether a task list that `TrackerSync` loaded reloads
/// it. The store sends those page requests itself. Each property is assigned only when its value changes, so an
/// unchanged poll redraws nothing and loading another page leaves the "Load older" button alone unless more pages
/// appear or run out.
@Observable
public final class WorklogHistoryStore: ChangeGuardedStore {
    public private(set) var worklogs: [WorklogItem] = []
    public private(set) var hasMore = false
    public private(set) var unavailable = false
    public private(set) var isLoading = false
    /// The history panel's message. It also shows connection failures, which tell the user why the list may be
    /// out of date.
    public private(set) var error: String?
    // Only the page requests read the cursor and the requested task, so views don't observe them.
    @ObservationIgnored private(set) var nextCursor: String?
    @ObservationIgnored private var taskID: String?
    /// Receives a failed page request's error, so that `TrackerSync` can recover the connection. It isn't called for a
    /// failure whose task is no longer selected. `TrackerFeatures` sets it once, after it creates `TrackerSync`, which
    /// needs this store.
    @ObservationIgnored var recordReadFailure: @MainActor (Error) -> Void = { _ in }
    private let client: any TrackerClient
    private let catalog: TaskCatalogStore
    private let connection: ConnectionStore

    init(client: any TrackerClient, catalog: TaskCatalogStore, connection: ConnectionStore) {
        self.client = client
        self.catalog = catalog
        self.connection = connection
    }

    /// Starts over with the selected task's first page. Requests are never cancelled.
    public func reload() {
        let selectedTaskID = catalog.selectedTaskID
        request(selectedTaskID: selectedTaskID)
        guard let selectedTaskID else { return }
        load(taskID: selectedTaskID, cursor: nil)
    }

    /// Requests the page after the loaded ones. Does nothing while the connection is stale, a page is loading or none
    /// is left.
    public func loadOlder() {
        guard !connection.stale, !isLoading, let taskID, let nextCursor else { return }
        load(taskID: taskID, cursor: nextCursor)
    }

    func request(selectedTaskID: String?) {
        if taskID != selectedTaskID {
            set(\.worklogs, [])
            setCursor(nil)
        }
        taskID = selectedTaskID
        set(\.unavailable, false)
        set(\.error, nil)
        set(\.isLoading, selectedTaskID != nil)
    }

    func beginLoading() {
        set(\.isLoading, true)
    }

    func fail(_ failure: Error) {
        set(\.isLoading, false)
        set(\.unavailable, true)
        set(\.error, failure.localizedDescription)
    }

    func accept(_ page: HistoryPage, cursor: String?) {
        set(\.isLoading, false)
        set(\.worklogs, cursor == nil || page.reset ? page.worklogs : worklogs + page.worklogs)
        setCursor(page.nextCursor)
        set(\.unavailable, false)
        set(\.error, nil)
    }

    /// Takes in a loaded task list. Reloads the history when the selected task or its running worklog changed, or
    /// when the last page request failed while the connection was stale, so that recovering the connection retries
    /// the history. Otherwise it clears a connection error, unless a failed page request shows its own error.
    func acceptTaskList(selectionChanged: Bool, connectionWasStale: Bool) {
        if selectionChanged || (connectionWasStale && unavailable) {
            reload()
        } else if !unavailable {
            set(\.error, nil)
        }
    }

    func showError(_ message: String) {
        set(\.error, message)
    }

    private func load(taskID requested: String, cursor: String?) {
        beginLoading()
        Task { [weak self] in
            guard let self else { return }
            do {
                let page = try await client.history(taskID: requested, cursor: cursor)
                // Drop a page whose task is no longer selected.
                guard requested == taskID else { return }
                accept(page, cursor: cursor)
            } catch {
                guard requested == taskID else { return }
                fail(error)
                recordReadFailure(error)
            }
        }
    }

    private func setCursor(_ cursor: String?) {
        nextCursor = cursor
        set(\.hasMore, cursor != nil)
    }
}
