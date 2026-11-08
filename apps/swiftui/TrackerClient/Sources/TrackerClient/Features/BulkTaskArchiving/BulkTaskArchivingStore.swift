import Foundation
import Observation

/// Holds the bulk archive dialog, loads its preview and sends its request.
///
/// The sheet and the sidebar toolbar read this store from the SwiftUI environment. Closing leaves the content in
/// place, because SwiftUI renders the sheet during its dismissal animation. `open` replaces it.
@Observable
public final class BulkTaskArchivingStore {
    private struct Search {
        let generation: Int
        let days: Int
        let asOf: String
    }

    public private(set) var isPresented = false
    public private(set) var daysText = "14"
    public private(set) var isLoading = false
    public private(set) var isSubmitting = false
    public private(set) var error: String?
    public private(set) var archivedCount: Int?
    private var preview: InactiveTaskPreview?
    private var generation = 0

    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let connection: ConnectionStore
    private let recordPreviewFailure: @MainActor (Error) -> Void
    private let reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void

    init(
        client: any TrackerClient, clock: any TrackerClock, connection: ConnectionStore,
        recordPreviewFailure: @escaping @MainActor (Error) -> Void,
        reloadAfterWrite: @escaping @MainActor (_ then: () -> Void) async -> Void
    ) {
        self.client = client
        self.clock = clock
        self.connection = connection
        self.recordPreviewFailure = recordPreviewFailure
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canOpen: Bool { connection.confirmed }
    public var hasPreview: Bool { preview != nil }
    public var tasks: [TaskItem] { preview?.tasks ?? [] }

    public var canSubmit: Bool {
        isPresented && !isLoading && !isSubmitting && archivedCount == nil
            && preview?.inactiveDays == days && !(preview?.tasks.isEmpty ?? true)
    }

    private var days: Int? {
        guard !daysText.isEmpty, daysText.allSatisfy(isDayDigit),
            let value = UInt32(daysText), value > 0
        else { return nil }
        return Int(value)
    }

    private func isDayDigit(_ character: Character) -> Bool { character.isASCII && character.isNumber }

    public func open() {
        guard canOpen, !isPresented else { return }
        daysText = "14"
        preview = nil
        isPresented = true
        refresh()
    }

    public func setDays(_ text: String) {
        guard isPresented, !isSubmitting, text != daysText else { return }
        daysText = text
        refresh()
    }

    /// Starts a new preview generation. Responses for older generations are ignored.
    public func refresh() {
        guard isPresented, !isSubmitting else { return }
        generation += 1
        if preview?.inactiveDays != days { preview = nil }
        archivedCount = nil
        error = days == nil ? "Enter a positive whole number of days." : nil
        isLoading = days != nil
        guard let days else { return }
        load(Search(generation: generation, days: days, asOf: formatTimestamp(clock.now)))
    }

    public func cancel() {
        guard !isSubmitting else { return }
        generation += 1
        isLoading = false
        isPresented = false
    }

    public func submit() {
        guard canSubmit, let preview else { return }
        isSubmitting = true
        error = nil
        Task { [weak self] in
            guard let self else { return }
            do {
                archivedCount = try await client.archiveInactiveTasks(preview: preview).archivedCount
            } catch {
                self.error = "\(writeFailure(error).localizedDescription) Refresh the preview before trying again."
            }
            self.preview = nil
            isSubmitting = false
            await reloadAfterWrite({})
        }
    }

    private func load(_ search: Search) {
        Task { [weak self] in
            guard let self else { return }
            do {
                let value = try await client.previewInactiveTasks(inactiveDays: search.days, asOf: search.asOf)
                try accept(value, search: search)
            } catch {
                guard generation == search.generation else { return }
                isLoading = false
                preview = nil
                self.error = error.localizedDescription
                recordPreviewFailure(error)
            }
        }
    }

    private func accept(_ value: InactiveTaskPreview, search: Search) throws {
        guard generation == search.generation else { return }
        guard value.inactiveDays == search.days, parseTimestamp(value.asOf) == parseTimestamp(search.asOf),
            parseTimestamp(value.asOf) != nil,
            Set(value.tasks.map(\.id)).count == value.tasks.count,
            value.tasks.allSatisfy({ !$0.archived })
        else {
            throw BridgeFailure(
                message: "The tracker returned an invalid archive preview.", kind: "protocol", requiresRefresh: true)
        }
        preview = value
        isLoading = false
    }
}
