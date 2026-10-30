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

final class BulkTaskArchivingState {
    struct Search {
        let generation: Int
        let days: Int
        let asOf: String
    }

    private(set) var isPresented = false
    private(set) var daysText = "14"
    private(set) var preview: InactiveTaskPreview?
    private(set) var error: String?
    private(set) var archivedCount: Int?
    private(set) var isLoading = false
    private(set) var isSubmitting = false
    private var generation = 0

    var days: Int? {
        guard !daysText.isEmpty, daysText.allSatisfy(isDayDigit),
            let value = UInt32(daysText), value > 0
        else { return nil }
        return Int(value)
    }

    private func isDayDigit(_ character: Character) -> Bool { character.isASCII && character.isNumber }
    var presentation: BulkTaskArchivingPresentation {
        BulkTaskArchivingPresentation(
            isPresented: isPresented, daysText: daysText,
            isLoading: isLoading, isSubmitting: isSubmitting, hasPreview: preview != nil,
            tasks: preview?.tasks ?? [],
            error: error, archivedCount: archivedCount,
            canSubmit: isPresented && !isLoading && !isSubmitting && archivedCount == nil
                && preview?.inactiveDays == days && !(preview?.tasks.isEmpty ?? true))
    }

    func open(asOf: String) -> Search? {
        guard !isPresented else { return nil }
        isPresented = true
        return requestPreview(asOf: asOf)
    }
    func updateDays(_ text: String, asOf: String) -> Search? {
        guard isPresented, !isSubmitting, text != daysText else { return nil }
        daysText = text
        return requestPreview(asOf: asOf)
    }
    /// Starts a new preview generation. Responses for older generations are ignored.
    func requestPreview(asOf: String) -> Search? {
        guard isPresented, !isSubmitting else { return nil }
        generation += 1
        if preview?.inactiveDays != days { preview = nil }
        archivedCount = nil
        error = days == nil ? "Enter a positive whole number of days." : nil
        isLoading = days != nil
        guard let days else { return nil }
        return Search(generation: generation, days: days, asOf: asOf)
    }
    func matches(_ search: Search) -> Bool { isPresented && generation == search.generation }
    func accept(_ value: InactiveTaskPreview, search: Search) throws {
        guard matches(search) else { return }
        guard value.inactiveDays == search.days, timestamp(value.asOf) == timestamp(search.asOf),
            timestamp(value.asOf) != nil,
            Set(value.tasks.map(\.id)).count == value.tasks.count,
            value.tasks.allSatisfy({ !$0.archived })
        else {
            throw BridgeFailure(
                message: "The tracker returned an invalid archive preview.", kind: "protocol", requiresRefresh: true)
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
    func submit() -> InactiveTaskPreview? {
        guard presentation.canSubmit, let preview else { return nil }
        isSubmitting = true
        error = nil
        return preview
    }
    func complete(count: Int) {
        isSubmitting = false
        preview = nil
        archivedCount = count
        error = nil
    }
    func failSubmission(_ failure: Error) {
        isSubmitting = false
        preview = nil
        error = "\(failure.localizedDescription) Refresh the preview before trying again."
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
    }
}
