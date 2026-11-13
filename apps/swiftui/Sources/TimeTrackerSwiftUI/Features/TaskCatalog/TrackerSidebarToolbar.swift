import SwiftUI
import TrackerClient

/// The toolbar's task list actions: new task and bulk archive.
struct TaskListToolbarActions: View {
    @Environment(TaskCreationStore.self) private var creation
    @Environment(BulkTaskArchivingStore.self) private var bulkArchiving

    var body: some View {
        Button {
            creation.open()
        } label: {
            Label("New task", systemImage: "plus")
        }
        .disabled(!creation.canOpen)
        .help("New task")

        Button {
            bulkArchiving.open()
        } label: {
            Label("Archive inactive tasks", systemImage: "archivebox")
        }
        .disabled(!bulkArchiving.canOpen)
        .help("Archive inactive tasks")
        .accessibilityIdentifier("bulk-archive.open")
    }
}
