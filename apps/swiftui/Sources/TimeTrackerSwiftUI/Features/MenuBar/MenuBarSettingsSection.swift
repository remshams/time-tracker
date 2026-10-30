import SwiftUI
import TrackerClient

struct MenuBarSettingsSection: View {
    let store: TrackerStore

    var body: some View {
        Section("Menu bar") {
            Toggle(
                "Show today's total next to the icon",
                isOn: Binding(
                    get: { store.showDailyTotalInMenuBar },
                    set: { store.setShowDailyTotalInMenuBar($0) }
                )
            )
            .accessibilityIdentifier("menu.show-daily-total")
            Picker(
                "Task order",
                selection: Binding(
                    get: { store.menuTaskOrder },
                    set: { store.setMenuTaskOrder($0) }
                )
            ) {
                ForEach(MenuTaskOrder.allCases) { order in
                    Text(order.title).tag(order)
                }
            }
            .accessibilityIdentifier("menu.task-order")
            Text("Tasks with no logged time appear last when ordering by time.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
