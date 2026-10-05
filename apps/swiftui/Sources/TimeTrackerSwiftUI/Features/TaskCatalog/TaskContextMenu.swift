import SwiftUI

struct TaskContextMenu: View {
    @ObservedObject var creation: TaskCreationStore
    @ObservedObject var rename: TaskRenameStore
    let taskID: String?

    var body: some View {
        Button("New task", action: creation.open)
            .disabled(!creation.canOpen)
        if let taskID {
            Button("Edit name") { rename.open(taskID: taskID) }
                .disabled(!rename.canOpen)
        }
    }
}
