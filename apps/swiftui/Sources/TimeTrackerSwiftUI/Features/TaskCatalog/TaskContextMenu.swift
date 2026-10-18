import SwiftUI

@MainActor
struct TaskContextMenu: View {
    let creation: TaskCreationStore
    let rename: TaskRenameStore
    let archiving: TaskArchivingStore
    let taskID: String?
    var taskIsArchived = false
    var taskIsRunning = false

    var body: some View {
        Button("New task") {
            guard creation.canOpen else { return }
            creation.open()
        }
        .disabled(!creation.canOpen)
        if let taskID {
            Button("Edit name") {
                guard rename.canOpen else { return }
                rename.open(taskID: taskID)
            }
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
