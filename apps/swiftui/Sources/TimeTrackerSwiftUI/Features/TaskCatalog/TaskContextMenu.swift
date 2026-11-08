import SwiftUI
import TrackerClient

struct TaskContextMenu: View {
    let taskID: String?
    var taskIsArchived = false
    var taskIsRunning = false
    @Environment(TaskCreationStore.self) private var creation
    @Environment(TaskRenameStore.self) private var rename
    @Environment(TaskArchivingStore.self) private var archiving
    @Environment(TrackerTaskPresentationCoordinator.self) private var presentation

    private var canCreate: Bool { creation.canOpen && !presentation.hasPresentation }
    private var canRename: Bool { rename.canOpen && !presentation.hasPresentation }

    var body: some View {
        Button("New task") {
            guard canCreate else { return }
            creation.open()
        }
        .disabled(!canCreate)
        if let taskID {
            Button("Edit name") {
                guard canRename else { return }
                rename.open(taskID: taskID)
            }
            .disabled(!canRename)

            Divider()
            if taskIsArchived {
                Button("Unarchive task") { archiving.unarchive(taskID: taskID) }
                    .disabled(!archiving.canUnarchive(taskID: taskID) || presentation.hasPresentation)
            } else {
                Button("Archive task...") { archiving.openArchive(taskID: taskID) }
                    .disabled(!archiving.canArchive(taskID: taskID) || presentation.hasPresentation)
                    .help(taskIsRunning ? "Stop tracking before archiving this task" : "Archive this task")
            }
        }
    }
}
