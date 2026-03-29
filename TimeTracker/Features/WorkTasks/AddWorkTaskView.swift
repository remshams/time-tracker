import SwiftUI

struct AddWorkTaskView: View {
  @Environment(\.dismiss) private var dismiss
  @State private var title = ""
  @State private var taskDescription = ""
  let onSave: (String, String) -> Void

  private var isSaveEnabled: Bool {
    !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  var body: some View {
    Form {
      TextField(
        String(localized: "add-task.title.label", defaultValue: "Title"),
        text: $title)
      TextField(
        String(localized: "add-task.description.label", defaultValue: "Description"),
        text: $taskDescription)
    }
    .padding()
    .frame(minWidth: 300)
    .toolbar {
      ToolbarItem(placement: .cancellationAction) {
        Button(String(localized: "add-task.cancel.button", defaultValue: "Cancel")) {
          dismiss()
        }
      }
      ToolbarItem(placement: .confirmationAction) {
        Button(String(localized: "add-task.save.button", defaultValue: "Save")) {
          onSave(title, taskDescription)
        }
        .disabled(!isSaveEnabled)
      }
    }
    .navigationTitle(String(localized: "add-task.navigation-title", defaultValue: "New Task"))
  }
}
