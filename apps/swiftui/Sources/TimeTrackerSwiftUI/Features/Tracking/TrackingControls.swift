import SwiftUI
import TrackerClient

struct TrackerElapsedText: View {
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        Text(readableDuration(clockText: tracking.timerText))
            .accessibilityLabel(tracking.timerText)
            .accessibilityIdentifier("tracking.elapsed")
    }
}

/// The one prominent start or stop button for a task, tinted in the task's color.
struct TrackingToggleButton: View {
    let taskID: String
    let availability: TrackingAvailability
    @Environment(TrackingStore.self) private var tracking

    var body: some View {
        let isRunning = tracking.active?.taskId == taskID
        let canTrack = isRunning ? availability.canStop : availability.canStart(taskID: taskID)
        Button {
            if let active = tracking.active, active.taskId == taskID {
                tracking.stopTracking(worklogID: active.id)
            } else {
                tracking.startTracking(taskID: taskID)
            }
        } label: {
            Label(
                isRunning ? "Stop tracking" : "Start tracking",
                systemImage: isRunning ? "stop.fill" : "play.fill"
            )
            .font(.headline)
            .padding(.horizontal, 10)
        }
        .buttonStyle(.glassProminent)
        .buttonBorderShape(.capsule)
        .controlSize(.extraLarge)
        .tint(TaskColor.forTaskID(taskID).swiftUIColor)
        .disabled(!canTrack)
        .help(
            isRunning
                ? "Stop tracking this task"
                : tracking.active == nil
                    ? "Start tracking this task" : "Stop the current timer and start tracking this task")
    }
}
