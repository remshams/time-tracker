import SwiftUI
import TrackerClient

@MainActor
struct TrackerMenuBarExtra: Scene {
    let store: TrackerStore
    let showTracker: () -> Void
    let quit: () -> Void

    var body: some Scene {
        MenuBarExtra {
            TrackerMenuPopup(store: store, showTracker: showTracker, quit: quit)
        } label: {
            TrackerMenuBarLabel(label: store.menu.label)
        }
        .menuBarExtraStyle(.window)
    }
}

private struct TrackerMenuBarLabel: View {
    @ObservedObject var label: TrackerMenuLabelStore

    var body: some View {
        let content = label.content
        HStack(spacing: 4) {
            TaskIndicatorDot(indicator: content.indicator)
            if let total = content.totalText {
                Text(total).monospacedDigit()
            }
        }
        .help(content.help)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(content.status)
        .accessibilityValue(content.totalText.map { "Total today: \($0)" } ?? "")
        .accessibilityHint("Open the menu to start or stop tracking.")
    }
}

private struct TrackerMenuPopup: View {
    let store: TrackerStore
    let showTracker: () -> Void
    let quit: () -> Void
    @ObservedObject private var menu: TrackerMenuStore
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase
    @State private var isVisible = false
    @State private var isOpen = false

    init(store: TrackerStore, showTracker: @escaping () -> Void, quit: @escaping () -> Void) {
        self.store = store
        self.showTracker = showTracker
        self.quit = quit
        menu = store.menu
    }

    var body: some View {
        let content = menu.content
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                trackingStatus(content)
                Divider()
                today(content)
                if !content.otherTasks.isEmpty {
                    DisclosureGroup("Start tracking") {
                        VStack(spacing: 2) {
                            ForEach(content.otherTasks) { entry in
                                taskButton(entry, content: content, showsDuration: false)
                            }
                        }
                        .padding(.top, 6)
                    }
                }
                Divider()
                HStack {
                    Button("Open Time Tracker") {
                        dismiss()
                        showTracker()
                    }
                    Spacer()
                    Button("Quit") { quit() }
                        .accessibilityLabel("Quit Time Tracker")
                }
            }
            .padding(16)
        }
        .frame(width: 360)
        .frame(maxHeight: 560)
        .fixedSize(horizontal: false, vertical: true)
        .onAppear {
            isVisible = true
            updatePresentation()
        }
        .onDisappear {
            isVisible = false
            updatePresentation()
        }
        .onChange(of: scenePhase) { _, _ in updatePresentation() }
    }

    private func updatePresentation() {
        let nextIsOpen = isVisible && scenePhase == .active
        guard nextIsOpen != isOpen else { return }
        isOpen = nextIsOpen
        if nextIsOpen { store.menuOpened() }
        else { store.menuClosed() }
    }

    private func trackingStatus(_ content: TrackerMenuContent) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(content.connectionStatusText)
                .font(.caption)
                .foregroundStyle(.secondary)
            if content.isStale {
                Text("Showing last confirmed state")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text(content.runningTaskName)
                .font(.headline)
                .lineLimit(3)
                .help(content.runningTaskName)
            if let status = content.autoPauseStatusText {
                Text(status).font(.caption)
            }
            if let error = content.trackingError {
                Text(error).font(.caption).foregroundStyle(.red)
            }
            if let elapsed = content.elapsedText {
                Text(elapsed).monospacedDigit()
            }
            if let worklogID = content.activeWorklogID {
                Button("Stop tracking", systemImage: "stop.fill") {
                    // The session checks the captured worklog against its current active worklog.
                    store.stopTracking(worklogID: worklogID)
                    dismiss()
                }
                .disabled(!content.canStopTracking)
            }
        }
    }

    private func today(_ content: TrackerMenuContent) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(content.dailyTotalsStatus == .cached ? "Today, cached" : "Today")
                .font(.headline)
            Text("Total today: \(content.totalText)")
                .monospacedDigit()
                .help(content.totalsExplanation)
            if content.todayTasks.isEmpty {
                Text(emptyTodayText(content.dailyTotalsStatus))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                VStack(spacing: 2) {
                    ForEach(content.todayTasks) { entry in
                        taskButton(entry, content: content, showsDuration: true)
                    }
                }
            }
        }
    }

    private func taskButton(_ entry: TrackerMenuTask, content: TrackerMenuContent,
                            showsDuration: Bool) -> some View {
        let isRunning = entry.isRunning && !content.isStale
        return Button {
            // Frozen rows carry identities; the session validates current task and connection state.
            store.startTracking(taskID: entry.id)
            dismiss()
        } label: {
            HStack(spacing: 8) {
                TaskIndicatorView(taskID: entry.id, isRunning: isRunning)
                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.task.name)
                        .fontWeight(isRunning ? .semibold : .regular)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                    if entry.task.archived {
                        Text("Archived").font(.caption).foregroundStyle(.secondary)
                    }
                }
                Spacer(minLength: 8)
                if showsDuration {
                    Text(entry.durationText).monospacedDigit().foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 6)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!entry.canStart && !isRunning)
        .help(showsDuration ? content.totalsExplanation : entry.task.name)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(taskAccessibilityLabel(entry, isStale: content.isStale,
                                                  showsDuration: showsDuration))
    }

    private func taskAccessibilityLabel(_ entry: TrackerMenuTask, isStale: Bool,
                                        showsDuration: Bool) -> String {
        let status: String
        if isStale {
            status = entry.isRunning ? "last confirmed tracking, current status unavailable"
                : "current tracking status unavailable"
        } else {
            status = entry.isRunning ? "tracking" : "not tracking"
        }
        let duration = showsDuration ? ", \(entry.durationText)" : ""
        let archived = entry.task.archived ? ", archived" : ""
        return "\(entry.task.name)\(duration)\(archived), \(status)"
    }

    private func emptyTodayText(_ status: DailyTotalsStatus) -> String {
        switch status {
        case .current: return "No time logged today"
        case .cached: return "Last confirmed totals are empty"
        case .loading: return "Loading today's totals"
        case .unavailable: return "Today's totals are unavailable"
        }
    }
}
