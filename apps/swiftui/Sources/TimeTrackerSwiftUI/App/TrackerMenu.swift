import AppKit
import SwiftUI
import TrackerClient

struct TrackerMenu: View {
    let store: TrackerStore
    @ObservedObject var menu: TrackerMenuStore
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let content = menu.content
        Text(content.connectionStatusText)
        if content.isStale { Text("Showing last confirmed state") }
        Text(content.runningTaskName)
        if let status = content.autoPauseStatusText { Text(status) }
        if let error = content.trackingError { Text(error) }
        if let elapsed = content.elapsedText { Text(elapsed).monospacedDigit() }
        if let worklogID = content.activeWorklogID {
            Button("Stop tracking") { store.stopTracking(worklogID: worklogID) }
                .disabled(!content.canStopTracking)
        }
        Divider()
        Text(content.dailyTotalsStatus == .cached ? "Today, cached" : "Today")
        Text("Total today: \(content.totalText)")
            .help(content.totalsExplanation)
        ForEach(content.todayTasks) { entry in
            TrackerDailyMenuEntry(store: store, entry: entry, explanation: content.totalsExplanation)
        }
        if content.todayTasks.isEmpty {
            switch content.dailyTotalsStatus {
            case .current: Text("No time logged today")
            case .cached: Text("Last confirmed totals are empty")
            case .loading: Text("Loading today's totals")
            case .unavailable: Text("Today's totals are unavailable")
            }
        }
        if !content.otherTasks.isEmpty {
            Menu("Start tracking") {
                ForEach(content.otherTasks) { entry in
                    Button(entry.isRunning ? "\(entry.task.name)  Running" : entry.task.name) {
                        store.startTracking(taskID: entry.id)
                    }
                    .disabled(!entry.canStart)
                }
            }
        }
        Divider()
        Button("Open Time Tracker") { openWindow(id: "tracker") }
        Button("Quit Time Tracker") { NSApplication.shared.terminate(nil) }
    }
}

private struct TrackerDailyMenuEntry: View {
    let store: TrackerStore
    let entry: TrackerMenuTask
    let explanation: String

    var body: some View {
        if entry.task.archived {
            Text("\(entry.task.name)  \(entry.durationText)  Archived")
                .help(explanation)
        } else {
            Button(label) { store.startTracking(taskID: entry.id) }
                .disabled(!entry.canStart)
                .help(explanation)
        }
    }

    private var label: String {
        let running = entry.isRunning ? "  Running" : ""
        return "\(entry.task.name)  \(entry.durationText)\(running)"
    }
}
