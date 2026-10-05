import SwiftUI

@MainActor
struct TaskContextMenu: View {
    @ObservedObject var creation: TaskCreationStore
    @ObservedObject var rename: TaskRenameStore
    let taskID: String?

    private var hasTaskEditor: Bool { creation.state.isPresented || rename.state.isPresented }

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
        }
    }
}
