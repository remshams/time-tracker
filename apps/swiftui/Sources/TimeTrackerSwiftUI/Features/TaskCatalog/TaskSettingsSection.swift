import SwiftUI
import TrackerClient

/// The Settings section for the task list. The sidebar and the menu bar both follow its task order.
struct TaskSettingsSection: View {
    @Environment(TaskCatalogStore.self) private var catalog

    var body: some View {
        Section("Tasks") {
            Picker(
                "Task order",
                selection: Binding(
                    get: { catalog.taskOrder },
                    set: { catalog.setTaskOrder($0) }
                )
            ) {
                ForEach(TaskOrder.allCases) { order in
                    Text(order.title).tag(order)
                }
            }
            .accessibilityIdentifier("settings.task-order")
            Text("Tasks with no logged time appear last when ordering by time.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
