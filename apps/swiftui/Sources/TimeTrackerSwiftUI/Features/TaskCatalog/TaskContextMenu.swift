import SwiftUI
import TrackerClient

struct TaskContextMenu: View {
    let taskID: String?
    var taskIsArchived = false
    var taskIsRunning = false
    var canStartTracking = false
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCreationStore.self) private var creation
    @Environment(TaskRenameStore.self) private var rename
    @Environment(TaskArchivingStore.self) private var archiving

    var body: some View {
        Button("New task") { creation.open() }
            .disabled(!creation.canOpen)
        if let taskID {
            if !taskIsArchived && !taskIsRunning {
                Button("Start Tracking") { tracking.startTracking(taskID: taskID) }
                    .disabled(!canStartTracking)
            }
            Button("Edit name") { rename.open(taskID: taskID) }
                .disabled(!rename.canOpen)

            Divider()
            if taskIsArchived {
                Button("Unarchive task") { archiving.unarchive(taskID: taskID) }
                    .disabled(!archiving.canUnarchive(taskID: taskID))
            } else {
                Button("Archive task...") { archiving.openArchive(taskID: taskID) }
                    .disabled(!archiving.canArchive(taskID: taskID))
                    .help(taskIsRunning ? "Stop tracking before archiving this task" : "Archive this task")
            }
        }
    }
}
