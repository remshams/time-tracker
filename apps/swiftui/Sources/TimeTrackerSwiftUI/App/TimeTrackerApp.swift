import AppKit
import SwiftUI
import TrackerClient

@main
@MainActor
struct TimeTrackerApp: App {
    @StateObject private var store = TrackerStore()
    @State private var menuBarInserted = true

    var body: some Scene {
        WindowGroup("Time Tracker", id: "tracker") {
            TrackerWindow(store: store)
        }
        .defaultSize(width: 1100, height: 760)
        .windowToolbarStyle(.unifiedCompact)

        MenuBarExtra(isInserted: $menuBarInserted) {
            TrackerMenu(store: store)
        } label: {
            TrackerMenuBarLabel(store: store, totals: store.dailyTotals)
        }

        Settings {
            ConnectionSettingsView(store: store)
        }
    }
}

private struct TrackerMenuBarLabel: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var totals: TrackerDailyTotalsStore

    var body: some View {
        if store.showDailyTotalInMenuBar {
            Label {
                Text(totals.menuBarText)
                    .monospacedDigit()
            } icon: {
                Image(systemName: menuBarSymbol)
            }
            .labelStyle(.titleAndIcon)
            .accessibilityLabel(Text("\(menuBarStatus). Total today: \(totals.totalText)"))
            .help("\(menuBarStatus)\nTotal today: \(totals.totalText)\n\(totals.explanation)")
        } else {
            Image(systemName: menuBarSymbol)
                .accessibilityLabel(Text(menuBarStatus))
                .help(menuBarStatus)
        }
    }

    private var menuBarSymbol: String {
        if store.isStale { return "questionmark.circle" }
        return store.active == nil ? "clock" : "play.circle.fill"
    }

    private var menuBarStatus: String {
        if store.isStale { return "Time Tracker: Tracking status unavailable" }
        if store.active != nil { return "Time Tracker: Tracking \(store.runningTaskName)" }
        return "Time Tracker: \(store.autoPauseStatusText ?? "No timer running")"
    }
}
