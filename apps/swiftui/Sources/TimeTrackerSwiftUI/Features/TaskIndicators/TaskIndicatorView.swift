import SwiftUI
import TrackerClient

/// The task's color mark in the sidebar: a dot with a halo while the task runs, a filled dot once it has time today,
/// and a ring otherwise.
struct TaskIndicatorView: View {
    let taskID: String
    let isRunning: Bool
    let hasTimeToday: Bool

    var body: some View {
        let color = ColorRole.task(TaskColor.forTaskID(taskID))
        ZStack {
            if isRunning {
                Circle()
                    .stroke(color.opacity(Emphasis.halo), lineWidth: BorderWidth.halo)
                    .frame(width: IconSize.regular - BorderWidth.halo, height: IconSize.regular - BorderWidth.halo)
            }
            if isRunning || hasTimeToday {
                Circle().fill(color).frame(width: IconSize.marker, height: IconSize.marker)
            } else {
                Circle().strokeBorder(color, lineWidth: BorderWidth.marker)
                    .frame(width: IconSize.marker, height: IconSize.marker)
            }
        }
        .frame(width: IconSize.regular, height: IconSize.regular)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(isRunning ? "Tracking" : "Not tracking")
        .accessibilityValue("Task color: \(TaskColor.forTaskID(taskID).rawValue)")
    }
}
