import SwiftUI
import TrackerClient

struct TrackerTaskToolbar: View {
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskArchivingStore.self) private var archiving
    @Environment(TaskRenameStore.self) private var rename
    @Environment(TaskCatalogStore.self) private var catalog

    var body: some View {
        if let task = catalog.selectedTask {
            Button {
                rename.open(taskID: task.id)
            } label: {
                Label("Edit task name", systemImage: "pencil")
            }
            .disabled(!rename.canOpen)
            .help("Edit task name")

            let canChangeArchive =
                task.archived
                ? archiving.canUnarchive(taskID: task.id)
                : archiving.canArchive(taskID: task.id)
            Button {
                if task.archived {
                    archiving.unarchive(taskID: task.id)
                } else {
                    archiving.openArchive(taskID: task.id)
                }
            } label: {
                Label(
                    task.archived ? "Unarchive task" : "Archive task",
                    systemImage: task.archived ? "archivebox.fill" : "archivebox")
            }
            .disabled(!canChangeArchive)
            .help(
                !task.archived && tracking.active?.taskId == task.id
                    ? "Stop tracking before archiving this task"
                    : task.archived ? "Unarchive this task" : "Archive this task")
        }
    }
}
