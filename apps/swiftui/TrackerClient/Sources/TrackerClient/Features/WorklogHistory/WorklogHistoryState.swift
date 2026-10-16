import Foundation

@MainActor
final class WorklogHistoryState {
    private(set) var worklogs: [WorklogItem] = []
    private(set) var nextCursor: String?
    var unavailable = false
    var error: String?
    private(set) var generation = 0
    private var taskID: String?
    var pending = false
    private(set) var isLoading = false

    func request(selectedTaskID: String?) {
        generation += 1
        if taskID != selectedTaskID {
            worklogs = []
            nextCursor = nil
        }
        taskID = selectedTaskID
        unavailable = false
        error = nil
        pending = selectedTaskID != nil
        isLoading = pending
    }

    func clearAfterCorrection() {
        worklogs = []
        nextCursor = nil
    }

    func removeWorklog(id: String) {
        worklogs.removeAll { $0.id == id }
        nextCursor = nil
    }

    func invalidateCursor() {
        nextCursor = nil
    }

    func beginLoading() {
        isLoading = true
    }

    func fail(_ failure: Error) {
        isLoading = false
        unavailable = true
        error = failure.localizedDescription
    }

    func invalidate() {
        generation += 1
        pending = false
        isLoading = false
    }

    func accept(_ page: HistoryPage, cursor: String?) {
        isLoading = false
        if cursor == nil || page.reset { worklogs = page.worklogs } else { worklogs.append(contentsOf: page.worklogs) }
        nextCursor = page.nextCursor
        unavailable = false
        error = nil
    }
}
