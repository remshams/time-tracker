import SwiftUI
import TrackerClient

/// The task's color mark in the sidebar. A filled dot means the task has time today, a ring means it has none, and a
/// dot with a halo marks the running task.
struct TaskIndicatorView: View {
    let taskID: String
    let isRunning: Bool
    var trackedToday = false

    private var color: Color { TaskColor.forTaskID(taskID).swiftUIColor }

    var body: some View {
        ZStack {
            if isRunning {
                Circle().stroke(color.opacity(0.35), lineWidth: 3).frame(width: 15, height: 15)
                Circle().fill(color).frame(width: 9, height: 9)
            } else if trackedToday {
                Circle().fill(color).frame(width: 9, height: 9)
            } else {
                Circle().strokeBorder(color, lineWidth: 1.5).frame(width: 9, height: 9)
            }
        }
        .frame(width: 16, height: 16)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(isRunning ? "Tracking" : "Not tracking")
        .accessibilityValue("Task color: \(TaskColor.forTaskID(taskID).rawValue)")
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
