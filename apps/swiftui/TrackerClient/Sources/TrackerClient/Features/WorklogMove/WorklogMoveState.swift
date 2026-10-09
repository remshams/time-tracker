import Foundation

public struct WorklogMovePresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let original: WorklogItem?
    public let sourceTaskName: String
    public let query: String
    public let candidates: [WorklogMoveCandidate]
    public let selectedTaskID: String?
    public let isSearching: Bool
    public let isSubmitting: Bool
    public let error: String?
    public let canEdit: Bool
    public let canSubmit: Bool
    public let requiresReview: Bool
    public let latest: WorklogItem?
}

@MainActor
final class WorklogMoveState {
    struct Intent {
        let expected: WorklogItem
        let destinationTaskID: String
        let historyPageLimit: Int

        func matchesReplacement(_ worklog: WorklogItem) -> Bool {
            worklog.id == expected.id && worklog.taskId == destinationTaskID
                && sameWorklogTimestamp(worklog.start, expected.start)
                && sameWorklogTimestamp(worklog.end, expected.end)
        }
    }

    struct Search {
        let revision: Int
        let sourceTaskID: String
        let query: String
        let refresh: Bool
    }

    private(set) var original: WorklogItem?
    private var sourceTaskName = ""
    private var query = ""
    private var candidates: [WorklogMoveCandidate] = []
    private var selectedTaskID: String?
    private var isPresented = false
    private var isSubmitting = false
    private var isSearching = false
    private var error: String?
    private var requiresReview = false
    private var latest: WorklogItem?
    private var revision = 0
    private var refreshNeeded = false
    private var historyPageLimit = 2
    private(set) var searchPending = false
    private(set) var intent: Intent?
    private(set) var pending = false
    private(set) var mayHaveCommitted = false

    var blocksConnectionChange: Bool { isPresented || intent != nil }
    var presentation: WorklogMovePresentation {
        let canEdit = isPresented && !isSubmitting && intent == nil && !requiresReview
        return WorklogMovePresentation(
            isPresented: isPresented, original: original,
            sourceTaskName: sourceTaskName, query: query, candidates: candidates,
            selectedTaskID: selectedTaskID, isSearching: isSearching, isSubmitting: isSubmitting,
            error: error, canEdit: canEdit,
            canSubmit: isPresented && !isSubmitting && !requiresReview
                && (intent != nil || (!isSearching && selectedTaskID != nil)),
            requiresReview: requiresReview, latest: latest)
    }

    func open(_ worklog: WorklogItem, sourceTaskName: String, historyPageLimit: Int) {
        if isPresented || intent != nil { isPresented = true; return }
        original = worklog
        self.sourceTaskName = sourceTaskName
        self.historyPageLimit = historyPageLimit
        isPresented = true
        refreshNeeded = true
        requestSearch()
    }

    private func requestSearch() {
        revision += 1
        candidates = []
        selectedTaskID = nil
        error = nil
        isSearching = true
        searchPending = true
    }

    func updateQuery(_ query: String) {
        guard presentation.canEdit, self.query != query else { return }
        self.query = query
        requestSearch()
    }

    func select(_ taskID: String) {
        guard presentation.canEdit, !isSearching, candidates.contains(where: { $0.id == taskID }) else { return }
        selectedTaskID = taskID
        error = nil
    }

    func moveSelection(by offset: Int) {
        guard presentation.canEdit, !isSearching, !candidates.isEmpty else { return }
        let index = candidates.firstIndex { $0.id == selectedTaskID } ?? 0
        selectedTaskID = candidates[min(max(index + offset, 0), candidates.count - 1)].id
    }

    func takeSearch() -> Search? {
        guard searchPending, isPresented, !requiresReview, intent == nil, let original else { return nil }
        searchPending = false
        return Search(revision: revision, sourceTaskID: original.taskId, query: query, refresh: refreshNeeded)
    }

    func retrySearch() {
        guard presentation.canEdit else { return }
        refreshNeeded = true
        requestSearch()
    }

    func currentSearch(replacing search: Search) -> Search? {
        guard isPresented, !requiresReview, intent == nil, original?.taskId == search.sourceTaskID else { return nil }
        return takeSearch() ?? search
    }

    func didRefresh(_ search: Search) { if original?.taskId == search.sourceTaskID { refreshNeeded = false } }
    func deferSearch() { if isPresented { searchPending = true } }

    func accept(_ values: [WorklogMoveCandidate], search: Search) {
        guard isPresented, !requiresReview, search.revision == revision else { return }
        candidates = values.filter { Self.isDestination($0, sourceTaskID: original?.taskId) }
        selectedTaskID = candidates.first?.id
        isSearching = false
        error = nil
    }

    private static func isDestination(_ candidate: WorklogMoveCandidate, sourceTaskID: String?) -> Bool {
        candidate.id != sourceTaskID
    }

    func failSearch(_ failure: Error, search: Search) {
        guard isPresented, !requiresReview, search.revision == revision else { return }
        candidates = []
        selectedTaskID = nil
        isSearching = false
        error = failure.localizedDescription
    }

    func submit() -> Bool {
        guard presentation.canSubmit, let original else { return false }
        if intent == nil {
            guard let selectedTaskID else { return false }
            intent = Intent(expected: original, destinationTaskID: selectedTaskID, historyPageLimit: historyPageLimit)
        }
        isSubmitting = true
        pending = true
        error = nil
        return true
    }

    func takePendingIntent() -> Intent? {
        guard pending, let intent else { return nil }
        pending = false
        return intent
    }
    func deferUntilWake() { pending = true }
    func commandStarted() { mayHaveCommitted = true }

    func fail(_ failure: Error, retainIntent: Bool) {
        isSubmitting = false
        pending = false
        error = failure.localizedDescription
        if !retainIntent { intent = nil; mayHaveCommitted = false }
    }

    func requireReview(latest: WorklogItem?, message: String) {
        self.latest = latest
        requiresReview = true
        revision += 1
        searchPending = false
        isSearching = false
        isSubmitting = false
        pending = false
        intent = nil
        mayHaveCommitted = false
        error = message
    }

    func reviewLatest(taskName: String?) {
        guard requiresReview, let latest else { return }
        original = latest
        if let taskName { sourceTaskName = taskName }
        self.latest = nil
        requiresReview = false
        refreshNeeded = true
        requestSearch()
    }

    func observe(_ worklog: WorklogItem) {
        guard isPresented, !isSubmitting, intent == nil, let original,
            worklog.id == original.id
        else { return }
        if worklog.taskId == original.taskId && sameWorklogTimestamp(worklog.start, original.start)
            && sameWorklogTimestamp(worklog.end, original.end)
        {
            return
        }
        requireReview(latest: worklog, message: "This worklog changed. Review it before moving.")
    }

    func cancel() {
        guard !isSubmitting else { return }
        if intent != nil { isPresented = false } else { reset() }
    }

    func reset() {
        original = nil
        sourceTaskName = ""
        query = ""
        candidates = []
        selectedTaskID = nil
        isPresented = false
        isSubmitting = false
        isSearching = false
        error = nil
        requiresReview = false
        latest = nil
        revision += 1
        searchPending = false
        refreshNeeded = false
        intent = nil
        mayHaveCommitted = false
        pending = false
    }
}
