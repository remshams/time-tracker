import AppKit
import SwiftUI
import TrackerClient

@main
@MainActor
struct TimeTrackerApp: App {
    @StateObject private var model = TrackerAppModel()
    @State private var menuBarInserted = true

    var body: some Scene {
        let store = model.store
        WindowGroup("Time Tracker", id: "tracker") {
            TrackerWindow(store: store)
        }
        .defaultSize(width: 1100, height: 760)
        .windowToolbarStyle(.unifiedCompact)
        .commands { TaskCreationCommands() }

        MenuBarExtra(isInserted: $menuBarInserted) {
            TrackerMenu(store: store, menu: store.menu)
        } label: {
            TrackerMenuBarLabel(label: store.menu.label)
        }

        Settings {
            ConnectionSettingsView(store: store)
        }
    }
}

@MainActor
private final class TrackerAppModel: ObservableObject {
    // Views observe their own adapters. Session updates must not rebuild the scenes.
    let store = TrackerStore()
}

private struct TrackerMenuBarLabel: View {
    @ObservedObject var label: TrackerMenuLabelStore

    var body: some View {
        let content = label.content
        if let total = content.totalText {
            Label {
                Text(total)
                    .monospacedDigit()
            } icon: {
                Image(systemName: content.symbol)
            }
            .labelStyle(.titleAndIcon)
            .accessibilityLabel(Text("\(content.status). Total today: \(total)"))
            .help(content.help)
        } else {
            Image(systemName: content.symbol)
                .accessibilityLabel(Text(content.status))
                .help(content.help)
        }
    }
}
