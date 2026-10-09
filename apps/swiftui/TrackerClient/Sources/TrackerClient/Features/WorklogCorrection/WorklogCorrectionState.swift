import Foundation

public struct WorklogCorrectionPresentation: Equatable, Sendable {
    public let isPresented: Bool
    public let taskName: String
    public let original: WorklogItem?
    public let start: Date
    public let end: Date?
    public let timezoneIdentifier: String
    public let isSubmitting: Bool
    public let error: String?
    public let canEdit: Bool
    public let canSubmit: Bool
    public let requiresReview: Bool
    public let latest: WorklogItem?
}

@MainActor
final class WorklogCorrectionState {
    struct Intent {
        let expected: WorklogItem
        let replacementStart: String
        let replacementEnd: String?
        let occurredAt: String
        let historyPageLimit: Int

        func matchesReplacement(_ worklog: WorklogItem) -> Bool {
            worklog.id == expected.id && worklog.taskId == expected.taskId
                && sameWorklogTimestamp(worklog.start, replacementStart)
                && sameWorklogTimestamp(worklog.end, replacementEnd)
        }
    }

    private(set) var original: WorklogItem?
    private var taskName = ""
    private var start = Date(timeIntervalSince1970: 0)
    private var end: Date?
    private var timezoneIdentifier = TimeZone.current.identifier
    private var calendar = Calendar(identifier: .gregorian)
    private var historyPageLimit = 2
    private var isPresented = false
    private var isSubmitting = false
    private var error: String?
    private var requiresReview = false
    private var latest: WorklogItem?
    private(set) var intent: Intent?
    private(set) var pending = false

    var presentation: WorklogCorrectionPresentation {
        let changed = original.map { hasChangedTimes(from: $0) } ?? false
        return WorklogCorrectionPresentation(
            isPresented: isPresented, taskName: taskName, original: original,
            start: timestamp(replacementStart) ?? start,
            end: timestamp(replacementEnd), timezoneIdentifier: timezoneIdentifier,
            isSubmitting: isSubmitting, error: error,
            canEdit: isPresented && !isSubmitting && intent == nil && !requiresReview,
            canSubmit: isPresented && !isSubmitting && !requiresReview && changed,
            requiresReview: requiresReview, latest: latest)
    }

    var blocksConnectionChange: Bool { isPresented || intent != nil }

    private func minute(_ date: Date) -> Date {
        let second = calendar.component(.second, from: date)
        let interval = date.timeIntervalSinceReferenceDate
        let fraction = interval - floor(interval)
        return date.addingTimeInterval(-Double(second) - fraction)
    }

    private var replacementStart: String {
        guard let original, let date = timestamp(original.start), minute(start) == minute(date) else {
            return commandTimestamp(minute(start))
        }
        return original.start
    }

    private var replacementEnd: String? {
        guard let end else { return nil }
        if let value = original?.end, let date = timestamp(value), minute(end) == minute(date) { return value }
        return commandTimestamp(minute(end))
    }

    private func hasChangedTimes(from original: WorklogItem) -> Bool {
        replacementStart != original.start || replacementEnd != original.end
    }

    private static func isNotInFuture(_ value: Date, now: Date) -> Bool {
        value <= now
    }

    private static func isOrderedInterval(start: Date, end: Date) -> Bool {
        end >= start
    }

    func open(_ worklog: WorklogItem, taskName: String, timezone: TimeZone = .current, historyPageLimit: Int = 2) {
        if isPresented || intent != nil { isPresented = true; return }
        guard let start = timestamp(worklog.start),
            worklog.end == nil || timestamp(worklog.end) != nil
        else { return }
        original = worklog
        self.taskName = taskName
        self.start = start
        end = timestamp(worklog.end)
        timezoneIdentifier = timezone.identifier
        calendar.timeZone = timezone
        self.historyPageLimit = historyPageLimit
        isPresented = true
        error = nil
    }

    func updateStart(_ value: Date) {
        guard presentation.canEdit else { return }
        start = minute(value)
        error = nil
    }

    func updateEnd(_ value: Date) {
        guard presentation.canEdit, original?.end != nil else { return }
        end = minute(value)
        error = nil
    }

    func submit(at date: Date) -> Bool {
        guard presentation.canSubmit, let original else { return false }
        if intent == nil {
            guard let replacementDate = timestamp(replacementStart), Self.isNotInFuture(replacementDate, now: date)
            else {
                error = "Start must not be in the future."
                return false
            }
            if let replacementEnd, let endDate = timestamp(replacementEnd) {
                guard Self.isOrderedInterval(start: replacementDate, end: endDate) else {
                    error = "End must not be before Start."
                    return false
                }
                guard Self.isNotInFuture(endDate, now: date) else {
                    error = "End must not be in the future."
                    return false
                }
            }
            intent = Intent(
                expected: original, replacementStart: replacementStart,
                replacementEnd: replacementEnd, occurredAt: commandTimestamp(date),
                historyPageLimit: historyPageLimit)
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

    func fail(_ failure: Error, retainIntent: Bool) {
        isSubmitting = false
        pending = false
        error = failure.localizedDescription
        if !retainIntent { intent = nil }
    }

    func requireReview(latest: WorklogItem?, message: String) {
        self.latest = latest
        requiresReview = true
        isSubmitting = false
        pending = false
        intent = nil
        error = message
    }

    func reviewLatest(taskName: String?) {
        guard requiresReview, let latest, let latestStart = timestamp(latest.start),
            latest.end == nil || timestamp(latest.end) != nil
        else { return }
        let changedStart = replacementStart != original?.start
        let changedEnd = replacementEnd != original?.end
        original = latest
        if !changedStart { start = latestStart }
        if latest.end == nil { end = nil } else if !changedEnd || end == nil { end = timestamp(latest.end) }
        if let taskName { self.taskName = taskName }
        self.latest = nil
        requiresReview = false
        error = nil
    }

    func observe(_ worklog: WorklogItem) {
        guard isPresented, !isSubmitting, intent == nil, let original,
            worklog.id == original.id, worklog != original
        else { return }
        requireReview(latest: worklog, message: "This worklog changed. Review its latest times before saving.")
    }

    func cancel() {
        guard !isSubmitting else { return }
        if intent != nil { isPresented = false } else { reset() }
    }

    func reset() {
        original = nil
        taskName = ""
        start = Date(timeIntervalSince1970: 0)
        end = nil
        isPresented = false
        isSubmitting = false
        error = nil
        requiresReview = false
        latest = nil
        intent = nil
        pending = false
    }
}
