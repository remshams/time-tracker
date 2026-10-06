import SwiftUI

@MainActor
struct TaskContextMenu: View {
    @ObservedObject var creation: TaskCreationStore
    @ObservedObject var rename: TaskRenameStore
    @ObservedObject var archiving: TaskArchivingStore
    let taskID: String?
    var taskIsArchived = false
    var taskIsRunning = false

    private var hasTaskEditor: Bool {
        creation.state.isPresented || rename.state.isPresented || archiving.state.isPresented
    }

    var body: some View {
        Button("New task") {
            guard creation.canOpen, !hasTaskEditor else { return }
            creation.open()
        }
        .disabled(!creation.canOpen || hasTaskEditor)
        if let taskID {
            Button("Edit name") {
                guard rename.canOpen, !hasTaskEditor else { return }
                rename.open(taskID: taskID)
            }
            .disabled(!rename.canOpen || hasTaskEditor)

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
