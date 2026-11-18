import Foundation

/// The line under the task header's name. It shows today's total while the task has time today or runs, and otherwise
/// when the task last ran. It doesn't say that the task runs or is archived, because the pill, the row and the tab
/// already do.
public enum TaskHeaderSubtitle: Equatable, Sendable {
    /// Today's total, shown as "0:45:12 today".
    case today(DurationText)
    /// When the task last ran, such as "Last tracked yesterday" or "Not tracked yet".
    case lastTracked(String)

    /// Builds the subtitle from the task's daily total, which is `nil` or has no seconds while today's totals are
    /// unavailable. A running task shows its total from its first second.
    public init(total: TaskDailyTotalPresentation?, isRunning: Bool, lastTracked: LastTracked) {
        if let total, let duration = total.duration, total.hasTime || isRunning {
            self = .today(duration)
            return
        }
        switch lastTracked.day {
        case .today(let time): self = .lastTracked("Last tracked today at \(time)")
        case .yesterday: self = .lastTracked("Last tracked yesterday")
        case .daysAgo(let days): self = .lastTracked("Last tracked \(days) days ago")
        case .date(let date): self = .lastTracked("Last tracked on \(date)")
        case .never: self = .lastTracked("Not tracked yet")
        }
    }

    /// The whole line as one string, for VoiceOver and the native E2E tests.
    public var text: String {
        switch self {
        case .today(let duration): "\(duration.hoursAndMinutes)\(duration.seconds) today"
        case .lastTracked(let text): text
        }
    }
}
