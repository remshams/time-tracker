import Foundation
import Observation

/// Holds the worklog correction dialog and sends its request.
///
/// The sheet and the worklog menus read this store from the SwiftUI environment. Closing leaves the content in
/// place, because SwiftUI renders the sheet during its dismissal animation. `open` replaces it.
@Observable
public final class WorklogCorrectionStore {
    public private(set) var isPresented = false {
        didSet { gate.update(self, isShown: isPresented) }
    }
    public private(set) var taskName = ""
    public private(set) var isSubmitting = false
    public private(set) var error: String?
    private(set) var original: WorklogItem?
    private var draftStart = Date(timeIntervalSince1970: 0)
    private var draftEnd: Date?
    private var calendar = Calendar(identifier: .gregorian)

    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let history: WorklogHistoryStore
    private let tracking: TrackingStore
    private let gate: PresentationGate
    private let reloadAfterWrite: @MainActor () async -> Void

    init(
        client: any TrackerClient, clock: any TrackerClock, connection: ConnectionStore, catalog: TaskCatalogStore,
        history: WorklogHistoryStore, tracking: TrackingStore, gate: PresentationGate,
        reloadAfterWrite: @escaping @MainActor () async -> Void
    ) {
        self.client = client
        self.clock = clock
        self.connection = connection
        self.catalog = catalog
        self.history = history
        self.tracking = tracking
        self.gate = gate
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canOpen: Bool { connection.confirmed && !gate.isClaimed }

    public var canEdit: Bool { isPresented && !isSubmitting }

    public var canSubmit: Bool {
        guard canEdit, let original else { return false }
        return replacementStart != original.start || replacementEnd != original.end
    }

    public var start: Date { parseTimestamp(replacementStart) ?? draftStart }

    public var end: Date? { parseTimestamp(replacementEnd) }

    public var timezone: TimeZone { calendar.timeZone }

    public func open(worklogID: String, timezone: TimeZone = .current) {
        guard canOpen, let worklog = worklog(withID: worklogID),
            let start = parseTimestamp(worklog.start), worklog.end == nil || parseTimestamp(worklog.end) != nil
        else { return }
        original = worklog
        taskName = catalog.tasks.first { $0.id == worklog.taskId }?.name ?? "Unknown task"
        draftStart = start
        draftEnd = parseTimestamp(worklog.end)
        calendar.timeZone = timezone
        error = nil
        isPresented = true
    }

    public func setStart(_ value: Date) {
        guard canEdit else { return }
        draftStart = minute(value)
        error = nil
    }

    public func setEnd(_ value: Date) {
        guard canEdit, original?.end != nil else { return }
        draftEnd = minute(value)
        error = nil
    }

    public func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
    }

    public func submit() {
        guard canSubmit, let original else { return }
        let now = clock.now
        if let message = validationError(at: now) {
            error = message
            return
        }
        isSubmitting = true
        error = nil
        Task { [weak self] in
            guard let self else { return }
            do {
                _ = try await client.correctWorklog(
                    expected: original, replacementStart: replacementStart, replacementEnd: replacementEnd,
                    occurredAt: formatTimestamp(now))
                isPresented = false
            } catch {
                self.error = writeFailure(error).localizedDescription
            }
            isSubmitting = false
            await reloadAfterWrite()
        }
    }

    private func worklog(withID id: String) -> WorklogItem? {
        history.worklogs.first { $0.id == id } ?? (tracking.active?.id == id ? tracking.active : nil)
    }

    private func minute(_ date: Date) -> Date {
        let second = calendar.component(.second, from: date)
        let interval = date.timeIntervalSinceReferenceDate
        let fraction = interval - floor(interval)
        return date.addingTimeInterval(-Double(second) - fraction)
    }

    private var replacementStart: String {
        guard let original, let date = parseTimestamp(original.start), minute(draftStart) == minute(date) else {
            return formatTimestamp(minute(draftStart))
        }
        return original.start
    }

    private var replacementEnd: String? {
        guard let draftEnd else { return nil }
        if let value = original?.end, let date = parseTimestamp(value), minute(draftEnd) == minute(date) {
            return value
        }
        return formatTimestamp(minute(draftEnd))
    }

    private func validationError(at now: Date) -> String? {
        guard start <= now else { return "Start must not be in the future." }
        guard let end, let replacementEnd else { return nil }
        guard hasPositiveWorklogDuration(start: replacementStart, end: replacementEnd) else {
            return "End must be after Start."
        }
        guard end <= now else { return "End must not be in the future." }
        return nil
    }
}
