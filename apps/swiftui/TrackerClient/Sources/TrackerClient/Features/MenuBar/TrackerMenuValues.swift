import Foundation

public struct TrackerMenuValues: Equatable, Sendable {
    public let elapsedText: MenuDurationText?
    public let totalText: MenuDurationText
    public let taskDurationTexts: [String: MenuDurationText]
    public let totalsExplanation: String

    init(_ inputs: TrackerMenuInputs, content: TrackerMenuContent) {
        let sameConnection = inputs.connectionSettings == content.connectionSettings
        if sameConnection, let worklogID = content.activeWorklogID, inputs.active?.id == worklogID {
            elapsedText = inputs.elapsedText
        } else {
            elapsedText = nil
        }
        totalText = sameConnection ? inputs.dayTotalText : MenuDurationText(placeholder: "Unavailable")
        taskDurationTexts = Dictionary(
            uniqueKeysWithValues: content.todayTasks.map {
                ($0.id, Self.taskDurationText(inputs, taskID: $0.id, sameConnection: sameConnection))
            })
        totalsExplanation =
            sameConnection
            ? inputs.totalsExplanation
            : "The connection changed. Reopen the menu to see the current tracker."
    }

    private static func taskDurationText(
        _ inputs: TrackerMenuInputs, taskID: String, sameConnection: Bool
    ) -> MenuDurationText {
        sameConnection ? inputs.durationText(taskID: taskID) : MenuDurationText(placeholder: "-")
    }
}
