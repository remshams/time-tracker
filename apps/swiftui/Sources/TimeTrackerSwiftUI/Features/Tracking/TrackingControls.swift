import SwiftUI

struct TrackingTaskButton: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var activity: TrackerActivityStore
    let taskID: String

    var body: some View {
        if let active = store.active, active.taskId == taskID {
            Button { store.stopTracking(worklogID: active.id) } label: {
                Label("Stop tracking", systemImage: "stop.fill")
            }
            .labelStyle(.iconOnly)
            .disabled(!activity.canStopTracking)
            .help("Stop tracking this task")
        } else {
            Button { store.startTracking(taskID: taskID) } label: {
                Label("Start tracking", systemImage: "play.fill")
            }
            .labelStyle(.iconOnly)
            .disabled(!activity.canStartTracking(taskID: taskID))
            .help(store.active == nil ? "Start tracking this task"
                  : "Stop the current timer and start tracking this task")
        }
    }
}

struct TrackerElapsedText: View {
    @ObservedObject var timer: TrackerTimerStore

    var body: some View { Text(timer.text) }
}
