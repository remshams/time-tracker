import Foundation

public struct TrackerMenuValues: Equatable, Sendable {
    public let elapsedText: String?
    public let totalText: String
    public let taskDurationTexts: [String: String]
    public let totalsExplanation: String

    @MainActor
    init(_ session: TrackerSession, content: TrackerMenuContent) {
        let sameConnection = session.connectionSettings == content.connectionSettings
        if sameConnection, let worklogID = content.activeWorklogID, session.active?.id == worklogID {
            elapsedText = session.timerDisplayText
        } else {
            elapsedText = nil
        }
        let total = session.totalDailyDurationText
        totalText = sameConnection
            ? (session.dailyTotalsStatus == .cached ? "~\(total)" : total) : "Unavailable"
        taskDurationTexts = Dictionary(uniqueKeysWithValues: content.todayTasks.map {
            ($0.id, sameConnection ? session.dailyDuration(taskID: $0.id).map(clockDuration) ?? "-" : "-")
        })
        totalsExplanation = sameConnection ? session.dailyTotalsExplanation
            : "The connection changed. Reopen the menu to see the current tracker."
    }
}
