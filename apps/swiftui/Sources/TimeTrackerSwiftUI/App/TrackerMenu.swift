import AppKit
import SwiftUI
import TrackerClient

struct TrackerMenu: View {
    @ObservedObject var store: TrackerStore
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(store.connectionStatusText)
        if store.isStale { Text("Showing last confirmed state") }
        Text(store.runningTaskName)
        if let status = store.autoPauseStatusText { Text(status) }
        if let error = store.trackingError { Text(error) }
        if store.active != nil {
            TrackerElapsedText(timer: store.timer)
        }
        Divider()
        Text(store.dailyTotalsStatus == .cached ? "Today, cached" : "Today")
        ForEach(store.todayTasks) { task in
            TrackerDailyMenuEntry(totals: store.dailyTotals, task: task) {
                store.changeTab(task.archived ? .archived : .active)
                store.select(task.id)
                openWindow(id: "tracker")
            }
        }
        if store.todayTasks.isEmpty {
            switch store.dailyTotalsStatus {
            case .current: Text("No time logged today")
            case .cached: Text("Last confirmed totals are empty")
            case .loading: Text("Loading today's totals")
            case .unavailable: Text("Today's totals are unavailable")
            }
        }
        Divider()
        Button("Open Time Tracker") { openWindow(id: "tracker") }
        Button("Quit Time Tracker") { NSApplication.shared.terminate(nil) }
    }
}

private struct TrackerDailyMenuEntry: View {
    @ObservedObject var totals: TrackerDailyTotalsStore
    let task: TaskItem
    let openTask: () -> Void

    var body: some View {
        Button("\(task.name)  \(totals.text(taskID: task.id))", action: openTask)
            .help(totals.explanation)
    }
}
