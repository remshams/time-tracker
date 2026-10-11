import SwiftUI
import TrackerClient

@MainActor
struct TaskContextMenu: View {
    let session: TrackerSession
    @ObservedObject var creation: TaskCreationModel
    @ObservedObject var rename: TaskRenameModel
    @ObservedObject var archiving: TaskArchivingModel
    let taskID: String?
    var taskIsArchived = false
    var taskIsRunning = false

    private var hasTaskEditor: Bool {
        creation.state.isPresented || rename.state.isPresented || archiving.state.isPresented
    }

    var body: some View {
        Button("New task") {
            guard creation.canOpen, !hasTaskEditor else { return }
            session.openTaskCreation()
        }
        .disabled(!creation.canOpen || hasTaskEditor)
        if let taskID {
            Button("Edit name") {
                guard rename.canOpen, !hasTaskEditor else { return }
                session.openTaskRename(taskID: taskID)
            }
            .disabled(!rename.canOpen || hasTaskEditor)

            Divider()
            if taskIsArchived {
                Button("Unarchive task") { session.unarchiveTask(taskID: taskID) }
                    .disabled(!session.canUnarchiveTask(taskID: taskID))
            } else {
                Button("Archive task...") { session.openTaskArchive(taskID: taskID) }
                    .disabled(!session.canArchiveTask(taskID: taskID))
                    .help(taskIsRunning ? "Stop tracking before archiving this task" : "Archive this task")
            }
        }
    }
}
