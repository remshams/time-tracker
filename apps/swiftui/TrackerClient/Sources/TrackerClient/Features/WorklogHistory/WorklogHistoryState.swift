import Foundation

final class WorklogHistoryState {
    private(set) var worklogs: [WorklogItem] = []
    private(set) var nextCursor: String?
    var unavailable = false
    var error: String?
    private var taskID: String?
    private(set) var isLoading = false

    func request(selectedTaskID: String?) {
        if taskID != selectedTaskID {
            worklogs = []
            nextCursor = nil
        }
        taskID = selectedTaskID
        unavailable = false
        error = nil
        isLoading = selectedTaskID != nil
    }

    func beginLoading() {
        isLoading = true
    }

    func fail(_ failure: Error) {
        isLoading = false
        unavailable = true
        error = failure.localizedDescription
    }

    func accept(_ page: HistoryPage, cursor: String?) {
        isLoading = false
        if cursor == nil || page.reset { worklogs = page.worklogs } else { worklogs.append(contentsOf: page.worklogs) }
        nextCursor = page.nextCursor
        unavailable = false
        error = nil
    }
}
