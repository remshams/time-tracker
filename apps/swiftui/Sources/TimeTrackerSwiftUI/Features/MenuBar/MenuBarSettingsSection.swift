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
            Text(
                "Task submenus can copy the task name, today's exact time, or today's time rounded to the nearest 15 minutes."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
        }
    }
}
