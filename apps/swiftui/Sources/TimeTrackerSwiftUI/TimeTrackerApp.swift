import AppKit
import SwiftUI

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

        MenuBarExtra("Time Tracker", systemImage: "clock", isInserted: $menuBarInserted) {
            TrackerMenu(store: store)
        }
    }
}

private struct TrackerMenu: View {
    @ObservedObject var store: TrackerStore
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(store.runningTaskName)
        if let elapsed = store.elapsed {
            Text(clockDuration(elapsed))
        }
        Divider()
        Button("Open Time Tracker") { openWindow(id: "tracker") }
        Button("Quit Time Tracker") { NSApplication.shared.terminate(nil) }
    }
}

private struct TrackerWindow: View {
    @ObservedObject var store: TrackerStore

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { store.tab }, set: { store.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { store.selectedTaskID }, set: { store.select($0) })
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 16) {
                Image(systemName: "clock")
                Text(store.runningTaskName)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text(store.elapsed.map(clockDuration) ?? "Idle")
                    .monospacedDigit()
                    .font(.title3)
            }
            .padding(.horizontal, 20)
            .frame(height: 56)
            .foregroundStyle(.white)
            .background(Color(nsColor: .darkGray))

            NavigationSplitView {
                VStack(spacing: 0) {
                    Picker("Tasks", selection: tabBinding) {
                        ForEach(TaskTab.allCases) { tab in
                            Text(tab.rawValue).tag(tab)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .padding(12)

                    List(selection: selectionBinding) {
                        ForEach(store.visibleTasks) { task in
                            Label {
                                Text(task.name).lineLimit(1)
                            } icon: {
                                Image(systemName: store.active?.taskId == task.id
                                      ? "circle.fill" : "checklist")
                                    .foregroundColor(store.active?.taskId == task.id
                                                     ? .green : .secondary)
                            }
                            .tag(task.id)
                        }
                    }
                    .listStyle(.sidebar)
                    .overlay {
                        if store.visibleTasks.isEmpty {
                            VStack(spacing: 8) {
                                Image(systemName: "tray")
                                Text("No \(store.tab.rawValue.lowercased()) tasks")
                            }
                            .foregroundStyle(.secondary)
                        }
                    }
                }
                .navigationTitle("Tasks")
                .navigationSplitViewColumnWidth(min: 240, ideal: 320, max: 480)
            } detail: {
                TaskDetails(store: store)
            }
        }
        .frame(minWidth: 480, minHeight: 420)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            store.refresh()
        }
    }
}

private struct TaskDetails: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let task = store.selectedTask {
                    Text(task.name)
                        .font(.largeTitle)
                        .textSelection(.enabled)

                    if task.archived {
                        Text("Archived").foregroundStyle(.secondary)
                    } else if let active = store.active, active.taskId == task.id {
                        Text("Running since \(localTimestamp(active.start))")
                            .foregroundStyle(.secondary)
                        Text(clockDuration(store.elapsed ?? 0))
                            .font(.title)
                            .monospacedDigit()
                            .foregroundStyle(.green)
                    } else {
                        Text("Active").foregroundStyle(.secondary)
                    }

                    Divider()
                    Text("Worklogs").font(.title2)
                    errorView

                    if store.worklogs.isEmpty && store.error == nil {
                        Text("No worklogs yet.").foregroundStyle(.secondary)
                    }

                    ForEach(store.worklogs) { worklog in
                        WorklogRow(worklog: worklog, active: store.active, elapsed: store.elapsed)
                    }

                    if store.hasMoreHistory {
                        Button("Load older worklogs") { store.loadOlder() }
                            .padding(.top, 8)
                    }
                } else {
                    errorView
                    Label("Select a task", systemImage: "checklist")
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(24)
        }
        .navigationTitle(store.selectedTask?.name ?? "Time Tracker")
    }

    @ViewBuilder
    private var errorView: some View {
        if let error = store.error {
            VStack(alignment: .leading, spacing: 8) {
                Text(error).foregroundStyle(.red)
                if store.historyUnavailable {
                    Button("Retry") { store.retryHistory() }
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
        }
    }
}

private struct WorklogRow: View {
    let worklog: WorklogItem
    let active: WorklogItem?
    let elapsed: TimeInterval?

    private var duration: TimeInterval {
        if active?.id == worklog.id { return elapsed ?? 0 }
        guard let start = timestamp(worklog.start), let end = timestamp(worklog.end) else { return 0 }
        return max(0, end.timeIntervalSince(start))
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(localDay(worklog.start))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .lineLimit(2)
            }
            Spacer(minLength: 12)
            Text(clockDuration(duration))
                .monospacedDigit()
        }
        .padding(12)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
    }
}

private func localTimestamp(_ value: String) -> String {
    guard let date = timestamp(value) else { return value }
    return date.formatted(date: .omitted, time: .shortened)
}

private func localDay(_ value: String) -> String {
    guard let date = timestamp(value) else { return value }
    return date.formatted(date: .abbreviated, time: .omitted)
}
