import Foundation
import Observation

/// Holds the selected task's worklog pages and the history panel's loading and error state.
///
/// Views read the properties. `TrackerSync` asks the store to reload the selected task or load more, and the
/// store sends those page requests itself. Each property is assigned only when its value changes, so an
/// unchanged poll redraws nothing and loading another page leaves the "Load older" button alone unless more
/// pages appear or run out.
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
    private let client: any TrackerClient

    init(client: any TrackerClient) { self.client = client }

    /// Starts over with the selected task's first page. Requests are never cancelled: `failed` receives the error
    /// after a failure, and isn't called for a failure whose task is no longer selected.
    func reload(selectedTaskID: String?, failed: @escaping @MainActor (Error) -> Void) {
        request(selectedTaskID: selectedTaskID)
        guard let selectedTaskID else { return }
        load(taskID: selectedTaskID, cursor: nil, failed: failed)
    }

    /// Requests the page after the loaded ones. Does nothing when a page is loading or none is left.
    func loadMore(failed: @escaping @MainActor (Error) -> Void) {
        guard !isLoading, let taskID, let nextCursor else { return }
        load(taskID: taskID, cursor: nextCursor, failed: failed)
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

    func showError(_ message: String) {
        set(\.error, message)
    }

    func clearError() {
        set(\.error, nil)
    }

    private func load(taskID requested: String, cursor: String?, failed: @escaping @MainActor (Error) -> Void) {
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
                failed(error)
            }
        }
    }

    private func setCursor(_ cursor: String?) {
        nextCursor = cursor
        set(\.hasMore, cursor != nil)
    }
}
