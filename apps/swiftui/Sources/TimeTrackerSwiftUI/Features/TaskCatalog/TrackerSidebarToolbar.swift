import SwiftUI
import TrackerClient

/// New task and bulk archive, as the sidebar's two footer buttons.
struct TrackerSidebarToolbar: View {
    @Environment(TaskCreationStore.self) private var creation
    @Environment(BulkTaskArchivingStore.self) private var bulkArchiving

    var body: some View {
        Button {
            creation.open()
        } label: {
            Label("New task", systemImage: "plus")
        }
        .buttonStyle(.borderless)
        .labelStyle(.iconOnly)
        .frame(width: 26, height: 26)
        .contentShape(Rectangle())
        .disabled(!creation.canOpen)
        .help("New task")

        Button {
            bulkArchiving.open()
        } label: {
            Label("Archive inactive tasks", systemImage: "archivebox")
        }
        .buttonStyle(.borderless)
        .labelStyle(.iconOnly)
        .frame(width: 26, height: 26)
        .contentShape(Rectangle())
        .disabled(!bulkArchiving.canOpen)
        .help("Archive inactive tasks")
        .accessibilityIdentifier("bulk-archive.open")
    }
}
