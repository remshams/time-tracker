import Foundation
import Observation

/// Holds the selected task's worklog pages and the history panel's loading and error state.
///
/// Views read the properties; `TrackerSession` writes them. Each property is assigned only when its value
/// changes, so an unchanged poll redraws nothing and loading another page leaves the "Load older" button
/// alone unless more pages appear or run out.
@Observable
public final class WorklogHistoryStore: ChangeGuardedStore {
    public private(set) var worklogs: [WorklogItem] = []
    public private(set) var hasMore = false
    public private(set) var unavailable = false
    public private(set) var isLoading = false
    /// The history panel's message. It also shows connection failures until the Connection slice moves them.
    public private(set) var error: String?
    // Only the session's load code reads the cursor and the requested task, so views don't observe them.
    @ObservationIgnored private(set) var nextCursor: String?
    @ObservationIgnored private var taskID: String?

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

    private func setCursor(_ cursor: String?) {
        nextCursor = cursor
        set(\.hasMore, cursor != nil)
    }
}
