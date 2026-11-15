import SwiftUI
import TrackerClient

struct TaskIndicatorView: View {
    let taskID: String
    let isRunning: Bool

    var body: some View {
        TaskIndicatorDot(indicator: TaskIndicator(taskID: taskID, isRunning: isRunning))
            .frame(width: IconSize.regular)
            .accessibilityLabel(isRunning ? "Tracking" : "Not tracking")
            .accessibilityValue("Task color: \(TaskColor.forTaskID(taskID).rawValue)")
    }
}

struct TaskIndicatorDot: View {
    let indicator: TaskIndicator

    var body: some View {
        Image(systemName: indicator.symbol)
            .renderingMode(.original)
            .font(TextRole.taskMarker)
            .foregroundStyle(indicator.color.map(ColorRole.task) ?? .secondary)
    }
}
