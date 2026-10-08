import SwiftUI
import TrackerClient

struct TaskIndicatorView: View {
    let taskID: String
    let isRunning: Bool

    var body: some View {
        TaskIndicatorDot(indicator: TaskIndicator(taskID: taskID, isRunning: isRunning))
            .frame(width: 16)
            .accessibilityLabel(isRunning ? "Tracking" : "Not tracking")
            .accessibilityValue("Task color: \(TaskColor.forTaskID(taskID).rawValue)")
    }
}

struct TaskIndicatorDot: View {
    let indicator: TaskIndicator

    var body: some View {
        Image(systemName: indicator.symbol)
            .renderingMode(.original)
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(indicator.color?.swiftUIColor ?? .secondary)
    }
}

extension TaskColor {
    var swiftUIColor: Color {
        switch self {
        case .blue: return .blue
        case .teal: return .teal
        case .green: return .green
        case .orange: return .orange
        case .red: return .red
        case .purple: return .purple
        case .pink: return .pink
        }
    }
}
