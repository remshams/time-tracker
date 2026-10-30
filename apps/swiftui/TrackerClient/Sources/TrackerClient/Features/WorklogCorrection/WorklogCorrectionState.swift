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
}

final class WorklogCorrectionState {
    struct Request {
        let expected: WorklogItem
        let replacementStart: String
        let replacementEnd: String?
        let occurredAt: String
    }

    private(set) var original: WorklogItem?
    private var taskName = ""
    private var start = Date(timeIntervalSince1970: 0)
    private var end: Date?
    private var timezoneIdentifier = TimeZone.current.identifier
    private var calendar = Calendar(identifier: .gregorian)
    private var isPresented = false
    private var isSubmitting = false
    private var error: String?

    var presentation: WorklogCorrectionPresentation {
        let changed = original.map { hasChangedTimes(from: $0) } ?? false
        return WorklogCorrectionPresentation(
            isPresented: isPresented, taskName: taskName, original: original,
            start: timestamp(replacementStart) ?? start,
            end: timestamp(replacementEnd), timezoneIdentifier: timezoneIdentifier,
            isSubmitting: isSubmitting, error: error,
            canEdit: isPresented && !isSubmitting,
            canSubmit: isPresented && !isSubmitting && changed)
    }

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

    func open(_ worklog: WorklogItem, taskName: String, timezone: TimeZone = .current) {
        guard !isPresented, let start = timestamp(worklog.start),
            worklog.end == nil || timestamp(worklog.end) != nil
        else { return }
        original = worklog
        self.taskName = taskName
        self.start = start
        end = timestamp(worklog.end)
        timezoneIdentifier = timezone.identifier
        calendar.timeZone = timezone
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

    func submit(at date: Date) -> Request? {
        guard presentation.canSubmit, let original else { return nil }
        guard let replacementDate = timestamp(replacementStart), Self.isNotInFuture(replacementDate, now: date)
        else {
            error = "Start must not be in the future."
            return nil
        }
        if let replacementEnd, let endDate = timestamp(replacementEnd) {
            guard hasPositiveWorklogDuration(start: replacementStart, end: replacementEnd) else {
                error = "End must be after Start."
                return nil
            }
            guard Self.isNotInFuture(endDate, now: date) else {
                error = "End must not be in the future."
                return nil
            }
        }
        isSubmitting = true
        error = nil
        return Request(
            expected: original, replacementStart: replacementStart,
            replacementEnd: replacementEnd, occurredAt: commandTimestamp(date))
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
        taskName = ""
        start = Date(timeIntervalSince1970: 0)
        end = nil
        isPresented = false
        isSubmitting = false
        error = nil
    }
}
