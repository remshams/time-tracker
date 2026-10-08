import Foundation

public struct BulkTaskArchivingPresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let daysText: String
    public let isLoading: Bool
    public let isSubmitting: Bool
    public let hasPreview: Bool
    public let tasks: [TaskItem]
    public let error: String?
    public let archivedCount: Int?
    public let canSubmit: Bool
}

@MainActor
final class BulkTaskArchivingState {
    struct Search {
        let generation: Int
        let days: Int
        let asOf: String
    }

    struct Submission {
        let generation: Int
        let preview: InactiveTaskPreview
    }

    private(set) var isPresented = false
    private(set) var daysText = "14"
    private(set) var preview: InactiveTaskPreview?
    private(set) var error: String?
    private(set) var archivedCount: Int?
    private(set) var isLoading = false
    private(set) var isSubmitting = false
    private(set) var pendingSearch = false
    private(set) var pendingSubmit = false
    private var generation = 0
    private var preparedSubmission = false

    var ownsPresentation: Bool { isPresented || isSubmitting }
    var days: Int? {
        guard !daysText.isEmpty, daysText.allSatisfy({ $0.isASCII && $0.isNumber }),
              let value = UInt32(daysText), value > 0 else { return nil }
        return Int(value)
    }
    var presentation: BulkTaskArchivingPresentation {
        BulkTaskArchivingPresentation(isPresented: isPresented, daysText: daysText,
            isLoading: isLoading, isSubmitting: isSubmitting, hasPreview: preview != nil,
            tasks: preview?.tasks ?? [],
            error: error, archivedCount: archivedCount,
            canSubmit: isPresented && !isLoading && !isSubmitting && archivedCount == nil &&
                preview?.inactiveDays == days && !(preview?.tasks.isEmpty ?? true))
    }

    func open() { isPresented = true; requestPreview() }
    func updateDays(_ text: String) {
        guard isPresented, !isSubmitting, text != daysText else { return }
        daysText = text
        requestPreview()
    }
    func requestPreview() {
        guard isPresented, !isSubmitting else { return }
        generation += 1
        if preview?.inactiveDays != days { preview = nil }
        archivedCount = nil
        error = days == nil ? "Enter a positive whole number of days." : nil
        pendingSearch = days != nil
        isLoading = pendingSearch
    }
    func takeSearch(asOf: String) -> Search? {
        guard pendingSearch, let days else { return nil }
        pendingSearch = false
        return Search(generation: generation, days: days, asOf: asOf)
    }
    func matches(_ search: Search) -> Bool { isPresented && generation == search.generation }
    func accept(_ value: InactiveTaskPreview, search: Search) throws {
        guard matches(search) else { return }
        guard value.inactiveDays == search.days, timestamp(value.asOf) == timestamp(search.asOf),
              timestamp(value.asOf) != nil,
              Set(value.tasks.map(\.id)).count == value.tasks.count,
              value.tasks.allSatisfy({ !$0.archived }) else {
            throw BridgeFailure(message: "The tracker returned an invalid archive preview.", kind: "protocol", requiresRefresh: true)
        }
        preview = value
        isLoading = false
        error = nil
    }
    func failSearch(_ failure: Error, search: Search) {
        guard matches(search) else { return }
        isLoading = false
        preview = nil
        error = failure.localizedDescription
    }
    func submit() -> Bool {
        guard presentation.canSubmit else { return false }
        pendingSubmit = true
        isSubmitting = true
        error = nil
        return true
    }
    func takeSubmission() -> Submission? {
        guard pendingSubmit, let preview else { return nil }
        pendingSubmit = false
        preparedSubmission = true
        return Submission(generation: generation, preview: preview)
    }
    func beginSubmission(_ submission: Submission) -> Bool {
        guard isPresented, isSubmitting, preparedSubmission,
              submission.generation == generation else { return false }
        preparedSubmission = false
        return true
    }
    func complete(count: Int) {
        isSubmitting = false
        preview = nil
        archivedCount = count
        error = nil
    }
    func failSubmission(_ failure: Error) {
        isSubmitting = false
        pendingSubmit = false
        preview = nil
        error = "\(failure.localizedDescription) Refresh the preview before trying again."
    }
    func sleep() {
        guard isPresented, archivedCount == nil else { return }
        if pendingSubmit || preparedSubmission {
            pendingSubmit = false
            preparedSubmission = false
            isSubmitting = false
        }
        guard !isSubmitting else { return }
        preview = nil
        requestPreview()
    }
    func cancel() { guard !isSubmitting else { return }; reset() }
    func reset() {
        generation += 1
        isPresented = false
        daysText = "14"
        preview = nil
        error = nil
        archivedCount = nil
        isLoading = false
        isSubmitting = false
        pendingSearch = false
        pendingSubmit = false
        preparedSubmission = false
    }
}
