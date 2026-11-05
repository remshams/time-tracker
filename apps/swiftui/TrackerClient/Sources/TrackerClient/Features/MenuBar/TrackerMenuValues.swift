import Foundation

public struct TrackerMenuValues: Equatable, Sendable {
    public let elapsedText: String?
    public let totalText: String
    public let taskDurationTexts: [String: String]
    public let totalsExplanation: String

    init(_ inputs: TrackerMenuInputs, content: TrackerMenuContent) {
        let sameConnection = inputs.connectionSettings == content.connectionSettings
        if sameConnection, let worklogID = content.activeWorklogID, inputs.active?.id == worklogID {
            elapsedText = inputs.timerText
        } else {
            elapsedText = nil
        }
        let total = inputs.dayTotalText
        totalText =
            sameConnection
            ? (inputs.dailyTotalsStatus == .cached ? "~\(total)" : total) : "Unavailable"
        taskDurationTexts = Dictionary(
            uniqueKeysWithValues: content.todayTasks.map {
                ($0.id, Self.taskDurationText(inputs, taskID: $0.id, sameConnection: sameConnection))
            })
        totalsExplanation =
            sameConnection
            ? inputs.totalsExplanation
            : "The connection changed. Reopen the menu to see the current tracker."
    }

    private static func taskDurationText(_ inputs: TrackerMenuInputs, taskID: String, sameConnection: Bool) -> String {
        sameConnection ? inputs.durationText(taskID: taskID) : "-"
    }
}
