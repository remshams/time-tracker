import Foundation

@MainActor
final class WorklogHistoryState {
    private(set) var worklogs: [WorklogItem] = []
    private(set) var nextCursor: String?
    var unavailable = false
    var error: String?
    private(set) var generation = 0
    var pending = false

    func request(selectedTaskID: String?) {
        generation += 1
        worklogs = []
        nextCursor = nil
        unavailable = false
        error = nil
        pending = selectedTaskID != nil
    }

    func invalidate() {
        generation += 1
        pending = false
    }

    func accept(_ page: HistoryPage, cursor: String?) {
        if cursor == nil || page.reset { worklogs = page.worklogs }
        else { worklogs.append(contentsOf: page.worklogs) }
        nextCursor = page.nextCursor
        unavailable = false
        error = nil
    }
}
