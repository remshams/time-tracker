import SwiftUI
import TrackerClient

struct MenuBarSettingsSection: View {
    @Environment(MenuBarPreferencesStore.self) private var preferences

    var body: some View {
        Section("Menu bar") {
            Toggle(
                "Show today's total next to the icon",
                isOn: Binding(
                    get: { preferences.showDailyTotalInMenuBar },
                    set: { preferences.setShowDailyTotalInMenuBar($0) }
                )
            )
            .accessibilityIdentifier("menu.show-daily-total")
            Picker(
                "Task order",
                selection: Binding(
                    get: { preferences.menuTaskOrder },
                    set: { preferences.setMenuTaskOrder($0) }
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
