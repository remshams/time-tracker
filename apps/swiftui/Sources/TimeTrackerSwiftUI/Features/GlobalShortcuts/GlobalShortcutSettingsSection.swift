import SwiftUI
import TrackerClient

struct GlobalShortcutSettingsSection: View {
    @Environment(GlobalShortcutsStore.self) private var store

    var body: some View {
        Section("Global shortcuts") {
            ForEach(GlobalShortcutAction.allCases, id: \.self) { action in
                HStack {
                    Text(action.title)
                    Spacer()
                    ShortcutRecorder(shortcut: store.shortcuts[action]) { shortcut in
                        store.setShortcut(action, shortcut: shortcut)
                    }
                    .frame(width: 160, height: 28)
                    .accessibilityLabel("\(action.title) shortcut")
                    .accessibilityIdentifier(
                        action == .openMenu ? "global-shortcut.open-menu" : "global-shortcut.quick-track")
                }
            }
            if let error = store.error {
                Text(error).foregroundStyle(ColorRole.error)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("global-shortcut.error")
            }
            Text(
                "Click a shortcut and press its new keys. Escape cancels. Both shortcuts work from any app: Open menu opens the menu bar menu, Open Quick Track opens the Quick Track dialog."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            Button("Restore default shortcuts") { store.resetShortcuts() }
                .accessibilityIdentifier("global-shortcut.restore")
        }
    }
}
