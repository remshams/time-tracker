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
}

final class WorklogMoveState {
    struct Request {
        let expected: WorklogItem
        let destinationTaskID: String
    }

    struct Search {
        let revision: Int
        let sourceTaskID: String
        let query: String
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
    private var revision = 0

    var presentation: WorklogMovePresentation {
        let canEdit = isPresented && !isSubmitting
        return WorklogMovePresentation(
            isPresented: isPresented, original: original,
            sourceTaskName: sourceTaskName, query: query, candidates: candidates,
            selectedTaskID: selectedTaskID, isSearching: isSearching, isSubmitting: isSubmitting,
            error: error, canEdit: canEdit,
            canSubmit: canEdit && !isSearching && selectedTaskID != nil)
    }

    func open(_ worklog: WorklogItem, sourceTaskName: String) -> Search? {
        guard !isPresented else { return nil }
        original = worklog
        self.sourceTaskName = sourceTaskName
        isPresented = true
        return requestSearch()
    }

    /// Starts a new search revision. Results for older revisions are ignored.
    private func requestSearch() -> Search? {
        guard let original else { return nil }
        revision += 1
        selectedTaskID = nil
        error = nil
        isSearching = true
        return Search(revision: revision, sourceTaskID: original.taskId, query: query)
    }

    func updateQuery(_ query: String) -> Search? {
        guard presentation.canEdit, self.query != query else { return nil }
        self.query = query
        return requestSearch()
    }

    func retrySearch() -> Search? {
        guard presentation.canEdit else { return nil }
        return requestSearch()
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

    func accept(_ values: [WorklogMoveCandidate], search: Search) {
        guard isPresented, search.revision == revision else { return }
        candidates = values.filter { Self.isDestination($0, sourceTaskID: original?.taskId) }
        selectedTaskID = candidates.first?.id
        isSearching = false
        error = nil
    }

    private static func isDestination(_ candidate: WorklogMoveCandidate, sourceTaskID: String?) -> Bool {
        candidate.id != sourceTaskID
    }

    func failSearch(_ failure: Error, search: Search) {
        guard isPresented, search.revision == revision else { return }
        candidates = []
        selectedTaskID = nil
        isSearching = false
        error = failure.localizedDescription
    }

    func submit() -> Request? {
        guard presentation.canSubmit, let original, let selectedTaskID else { return nil }
        isSubmitting = true
        error = nil
        return Request(expected: original, destinationTaskID: selectedTaskID)
    }

    func fail(_ failure: Error) {
        isSubmitting = false
        error = failure.localizedDescription
    }

    func cancel() {
        guard !isSubmitting else { return }
        reset()
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
        revision += 1
    }
}
