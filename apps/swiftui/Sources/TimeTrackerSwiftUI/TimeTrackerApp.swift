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
        .windowToolbarStyle(.unifiedCompact)

        MenuBarExtra("Time Tracker", systemImage: "clock", isInserted: $menuBarInserted) {
            TrackerMenu(store: store)
        }

        Settings {
            ConnectionSettingsView(store: store)
        }
    }
}

private struct TrackerMenu: View {
    @ObservedObject var store: TrackerStore
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(store.connectionStatusText)
        if store.isStale { Text("Showing last confirmed state") }
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

    private var trackingFailurePresented: Binding<Bool> {
        Binding(get: { store.trackingError != nil }, set: {
            if !$0 { store.dismissTrackingError() }
        })
    }

    var body: some View {
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
                            Text(task.name)
                                .lineLimit(2)
                                .padding(.vertical, 4)
                        } icon: {
                            Image(systemName: store.active?.taskId == task.id
                                  ? "timer" : "checklist")
                        }
                        .tag(task.id)
                        .help(task.name)
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
            .navigationSplitViewColumnWidth(min: 240, ideal: 280, max: 400)
        } detail: {
            VStack(spacing: 0) {
                ConnectionSummary(store: store)
                Divider()
                TimerSummary(store: store)
                Divider()
                TaskDetails(store: store)
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        .alert("Could not change tracking", isPresented: trackingFailurePresented) {
            Button("OK", role: .cancel) { store.dismissTrackingError() }
        } message: {
            Text(store.trackingError ?? "Please try again.")
        }
    }
}

private struct ConnectionSummary: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: store.connectionSettings.mode == .local ? "internaldrive" : "network")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 3) {
                Text(store.connectionSettings.mode == .local
                     ? "Local database" : store.connectionSettings.serverURL)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                    .help(store.connectionSettings.serverURL)
                Text(store.connectionStatusText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .help(store.connectionMessage ?? store.connectionStatusText)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if store.isBusy {
                ProgressView().controlSize(.small)
            } else if store.isStale {
                Button("Retry") { store.refresh() }
            }
            ConnectionSettingsButton(store: store)
                .buttonStyle(.borderless)
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 12)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

private struct TimerSummary: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: store.active == nil ? "clock" : "timer")
                .font(.title2)
                .foregroundStyle(store.active == nil ? Color.secondary : Color.accentColor)

            VStack(alignment: .leading, spacing: 4) {
                Text(store.active == nil ? "Timer" : "Currently tracking")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(store.runningTaskName)
                    .font(.headline)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .help(store.runningTaskName)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            Text(store.timerDisplayText)
                .font(.system(.title2, design: .monospaced).weight(.medium))
                .monospacedDigit()
                .fixedSize()
        }
        .foregroundStyle(.primary)
        .padding(.horizontal, 24)
        .padding(.vertical, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: .controlBackgroundColor))
    }
}

private struct TaskDetails: View {
    @ObservedObject var store: TrackerStore

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let task = store.selectedTask {
                VStack(alignment: .leading, spacing: 10) {
                    Text(task.name)
                        .font(.system(size: 26, weight: .semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                        .help(task.name)

                    HStack(spacing: 12) {
                        if task.archived {
                            Label("Archived", systemImage: "archivebox")
                                .foregroundStyle(.secondary)
                        } else if let active = store.active, active.taskId == task.id {
                            Label("Running since \(localTimestamp(active.start))", systemImage: "timer")
                                .foregroundStyle(.secondary)
                        } else {
                            Label("Active", systemImage: "checklist")
                                .foregroundStyle(.secondary)
                        }

                        Spacer(minLength: 12)
                        if !task.archived {
                            trackingButton(taskID: task.id)
                                .fixedSize()
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(24)

                Divider()
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        Text("Worklogs")
                            .font(.title3.weight(.semibold))
                            .padding(.bottom, 4)
                        errorView

                        if store.worklogs.isEmpty && store.error == nil {
                            Label("No worklogs yet", systemImage: "clock")
                                .foregroundStyle(.secondary)
                                .padding(.vertical, 12)
                        }

                        ForEach(store.worklogs) { worklog in
                            WorklogRow(worklog: worklog, active: store.active, elapsed: store.elapsed)
                        }

                        if store.hasMoreHistory {
                            Button("Load older worklogs") { store.loadOlder() }
                                .disabled(store.isBusy || store.isStale)
                                .padding(.top, 8)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(24)
                }
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    errorView
                    Label("Select a task", systemImage: "checklist")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                }
                .padding(24)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(nsColor: .windowBackgroundColor))
        .navigationTitle(store.selectedTask?.name ?? "Time Tracker")
    }

    @ViewBuilder
    private func trackingButton(taskID: String) -> some View {
        if let active = store.active, active.taskId == taskID {
            Button { store.stopTracking(worklogID: active.id) } label: {
                Label("Stop tracking", systemImage: "stop.fill")
            }
            .buttonStyle(.bordered)
            .disabled(!store.canStopTracking)
            .help("Stop the running timer and save this worklog.")
        } else {
            Button { store.startTracking(taskID: taskID) } label: {
                Label(store.active == nil ? "Start tracking" : "Switch tracking", systemImage: "play.fill")
            }
            .buttonStyle(.borderedProminent)
            .disabled(!store.canStartSelectedTask)
            .help(store.active == nil
                  ? "Start a timer for this task."
                  : "Stop the current timer and start tracking this task.")
        }
    }

    @ViewBuilder
    private var errorView: some View {
        if let error = store.error {
            VStack(alignment: .leading, spacing: 8) {
                Label {
                    Text(error).textSelection(.enabled)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(Color(nsColor: .systemRed))
                }
                .foregroundStyle(.primary)
                if store.historyUnavailable {
                    Button("Retry") { store.retryHistory() }
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay {
                RoundedRectangle(cornerRadius: 8)
                    .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
            }
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
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.primary)
                Text("\(localTimestamp(worklog.start)) – \(worklog.end.map(localTimestamp) ?? "Running")")
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 12)
            Text(clockDuration(duration))
                .font(.system(.title3, design: .monospaced).weight(.medium))
                .foregroundStyle(.primary)
                .monospacedDigit()
                .fixedSize()
        }
        .padding(16)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay {
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
        }
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
