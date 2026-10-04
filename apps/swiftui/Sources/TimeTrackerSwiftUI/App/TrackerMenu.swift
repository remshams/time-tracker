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
        TrackerMenuStopAction(store: store, activity: store.activity)
        Divider()
        Text(store.dailyTotalsStatus == .cached ? "Today, cached" : "Today")
        TrackerMenuDailyTotal(totals: store.dailyTotals)
        ForEach(store.todayTasks) { task in
            TrackerDailyMenuEntry(store: store, totals: store.dailyTotals,
                                  activity: store.activity, task: task)
        }
        if store.todayTasks.isEmpty {
            switch store.dailyTotalsStatus {
            case .current: Text("No time logged today")
            case .cached: Text("Last confirmed totals are empty")
            case .loading: Text("Loading today's totals")
            case .unavailable: Text("Today's totals are unavailable")
            }
        }
        if !tasksWithoutTimeToday.isEmpty {
            Menu("Start tracking") {
                ForEach(tasksWithoutTimeToday) { task in
                    TrackerMenuStartAction(store: store, activity: store.activity, task: task)
                }
            }
        }
        Divider()
        Button("Open Time Tracker") { openWindow(id: "tracker") }
        Button("Quit Time Tracker") { NSApplication.shared.terminate(nil) }
    }

    private var tasksWithoutTimeToday: [TaskItem] {
        let todayIDs = Set(store.todayTasks.map(\.id))
        return store.tasks.filter { !$0.archived && !todayIDs.contains($0.id) }
            .sorted {
                if $0.name == $1.name { return $0.id < $1.id }
                return $0.name < $1.name
            }
    }
}

private struct TrackerMenuStopAction: View {
    let store: TrackerStore
    @ObservedObject var activity: TrackerActivityStore

    var body: some View {
        if let active = store.active {
            Button("Stop tracking") { store.stopTracking(worklogID: active.id) }
                .disabled(!activity.canStopTracking)
        }
    }
}

private struct TrackerMenuDailyTotal: View {
    @ObservedObject var totals: TrackerDailyTotalsStore

    var body: some View {
        Text("Total today: \(totals.totalText)")
            .help(totals.explanation)
    }
}

private struct TrackerDailyMenuEntry: View {
    let store: TrackerStore
    @ObservedObject var totals: TrackerDailyTotalsStore
    @ObservedObject var activity: TrackerActivityStore
    let task: TaskItem

    var body: some View {
        if task.archived {
            Text("\(task.name)  \(totals.text(taskID: task.id))  Archived")
                .help(totals.explanation)
        } else {
            Button(label) { store.startTracking(taskID: task.id) }
                .disabled(!activity.canStartTracking(taskID: task.id))
                .help(totals.explanation)
        }
    }

    private var label: String {
        let running = store.active?.taskId == task.id ? "  Running" : ""
        return "\(task.name)  \(totals.text(taskID: task.id))\(running)"
    }
}

private struct TrackerMenuStartAction: View {
    let store: TrackerStore
    @ObservedObject var activity: TrackerActivityStore
    let task: TaskItem

    var body: some View {
        Button(store.active?.taskId == task.id ? "\(task.name)  Running" : task.name) {
            store.startTracking(taskID: task.id)
        }
            .disabled(!activity.canStartTracking(taskID: task.id))
    }
}
