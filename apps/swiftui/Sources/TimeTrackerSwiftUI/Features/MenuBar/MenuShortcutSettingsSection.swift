import SwiftUI
import TrackerClient

struct MenuShortcutSettingsSection: View {
    @Environment(MenuBarPreferencesStore.self) private var preferences

    var body: some View {
        Section("Menu shortcut") {
            ForEach([MenuShortcutAction.openMenu], id: \.self) { action in
                HStack {
                    Text(action.title)
                    Spacer()
                    ShortcutRecorder(shortcut: preferences.menuShortcuts[action]) { shortcut in
                        preferences.setMenuShortcut(action, shortcut: shortcut)
                    }
                    .frame(width: 160, height: 28)
                    .accessibilityLabel("\(action.title) shortcut")
                    .accessibilityIdentifier("menu.shortcut.open")
                }
            }
            if let error = preferences.menuShortcutError {
                Text(error).foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("menu.shortcut.error")
            }
            Text(
                "Click the shortcut and press its new keys. Escape cancels. Open menu opens the native menu across apps. Use arrow keys to navigate and open task submenus for tracking and copying."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            Text(
                "Task submenus can copy the task name, today's exact time, or today's time rounded to the nearest 15 minutes."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            Button("Restore default shortcut") { preferences.resetMenuShortcuts() }
                .accessibilityIdentifier("menu.shortcut.restore")
        }
    }
}
