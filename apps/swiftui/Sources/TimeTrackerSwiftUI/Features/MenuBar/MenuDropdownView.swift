import AppKit
import SwiftUI
import TrackerClient

@MainActor
final class MenuDropdownState: ObservableObject {
    let content: TrackerMenuContent
    let connection: ConnectionSettings
    @Published private(set) var selection: MenuTaskSelection
    @Published var feedback: String?
    @Published private(set) var keyboardNavigation = 0
    var allowsTaskActivation = true

    init(content: TrackerMenuContent, connection: ConnectionSettings) {
        self.content = content
        self.connection = connection
        let entries = content.todayTasks + content.otherTasks
        selection = MenuTaskSelection(taskIDs: entries.map(\.id), initialTaskID: entries.first(where: \.isRunning)?.id)
    }

    func moveDown() { selection.moveDown(); revealKeyboardSelection() }
    func moveUp() { selection.moveUp(); revealKeyboardSelection() }
    func select(_ id: String) { selection.select(taskID: id); feedback = nil }

    private func revealKeyboardSelection() {
        keyboardNavigation += 1
        allowsTaskActivation = true
        feedback = nil
    }
}

@MainActor
struct MenuDropdownView: View {
    @ObservedObject var state: MenuDropdownState
    let shortcuts: MenuKeyboardShortcuts
    let activateTask: (String) -> Void
    let stopTracking: () -> Void
    let copyValue: (MenuShortcutAction) -> Void
    let openWindow: () -> Void
    let quit: () -> Void
    @FocusState private var focusedControl: Control?

    private enum Control: Hashable {
        case task(String), action(String)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(state.content.connectionStatusText).font(.caption).foregroundStyle(.secondary)
            if state.content.isStale { Text("Showing last confirmed state").foregroundStyle(.secondary) }
            Text(state.content.runningTaskName).fontWeight(.semibold)
            if let status = state.content.autoPauseStatusText { Text(status).font(.caption) }
            if let error = state.content.trackingError { Text(error).foregroundStyle(.red) }
            if let elapsed = state.content.elapsedText { Text(elapsed).monospacedDigit() }
            if state.content.activeWorklogID != nil {
                Button("Stop tracking", action: stopTracking).disabled(!state.content.canStopTracking)
                    .focused($focusedControl, equals: .action("stop"))
            }
            Divider()
            Text(state.content.dailyTotalsStatus == .cached ? "Today, cached" : "Today").font(.headline)
            Text("Total today: \(state.content.totalText)").monospacedDigit().help(state.content.totalsExplanation)
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(state.content.todayTasks) { entry in taskRow(entry, showsDuration: true) }
                        if state.content.todayTasks.isEmpty {
                            Text(emptyTodayText).foregroundStyle(.secondary).padding(.vertical, 4)
                        }
                        if !state.content.otherTasks.isEmpty {
                            Divider().padding(.vertical, 6)
                            Text("Start tracking").font(.headline).padding(.bottom, 4)
                            ForEach(state.content.otherTasks) { entry in taskRow(entry, showsDuration: false) }
                        }
                    }
                }
                .frame(maxHeight: 300)
                .onChange(of: state.keyboardNavigation) { _ in
                    if let id = state.selection.selectedTaskID {
                        focusedControl = .task(id)
                        proxy.scrollTo(id, anchor: .center)
                    }
                }
                .onAppear {
                    if let id = state.selection.selectedTaskID { proxy.scrollTo(id, anchor: .center) }
                }
            }
            Divider()
            HStack {
                ForEach([MenuShortcutAction.copyName, .copyExact, .copyRounded], id: \.self) { action in
                    Button(copyTitle(action)) { copyValue(action) }
                        .focused($focusedControl, equals: .action(action.rawValue))
                        .disabled(state.selection.selectedTaskID == nil)
                        .help("\(action.title): \(shortcuts[action].displayText)")
                }
            }
            Text(state.feedback ?? "↑/↓ or \(shortcuts.moveDown.displayText)/\(shortcuts.moveUp.displayText) to navigate. Return starts the highlighted task.")
                .font(.caption).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityLabel(state.feedback ?? "Task navigation shortcuts")
            HStack {
                Button("Open Time Tracker", action: openWindow)
                    .focused($focusedControl, equals: .action("open"))
                Spacer()
                Button("Quit", action: quit)
                    .focused($focusedControl, equals: .action("quit"))
            }
        }
        .buttonStyle(.borderless)
        .padding(16)
        .frame(width: 390)
        .onChange(of: focusedControl) { control in
            switch control {
            case .task(let id): state.allowsTaskActivation = true; state.select(id)
            case .action: state.allowsTaskActivation = false
            case nil: state.allowsTaskActivation = true
            }
        }
    }

    private func taskRow(_ entry: TrackerMenuTask, showsDuration: Bool) -> some View {
        let selected = state.selection.selectedTaskID == entry.id
        return Button {
            state.select(entry.id)
            activateTask(entry.id)
        } label: {
            HStack {
                TaskIndicatorView(taskID: entry.id, isRunning: entry.isRunning && !state.content.isStale)
                Text(entry.task.name).lineLimit(1).truncationMode(.middle)
                    .fontWeight(entry.isRunning ? .semibold : .regular)
                Spacer(minLength: 8)
                if showsDuration { Text(entry.durationText).monospacedDigit() }
                if entry.task.archived { Text("Archived").font(.caption).foregroundStyle(.secondary) }
            }
            .padding(.horizontal, 8).padding(.vertical, 6)
            .contentShape(Rectangle())
            .background(selected ? Color.accentColor.opacity(0.2) : Color.clear)
            .clipShape(RoundedRectangle(cornerRadius: 5))
        }
        .id(entry.id)
        .focused($focusedControl, equals: .task(entry.id))
        .onHover { hovering in if hovering { state.select(entry.id) } }
        .help(entry.canStart ? "Start tracking \(entry.task.name)" : "Select \(entry.task.name) to copy its time")
        .accessibilityLabel("\(entry.task.name), \(entry.durationText)\(entry.task.archived ? ", archived" : "")")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var emptyTodayText: String {
        switch state.content.dailyTotalsStatus {
        case .current: return "No time logged today"
        case .cached: return "Last confirmed totals are empty"
        case .loading: return "Loading today's totals"
        case .unavailable: return "Today's totals are unavailable"
        }
    }

    private func copyTitle(_ action: MenuShortcutAction) -> String {
        switch action {
        case .copyName: return "Name [\(shortcuts[action].displayText)]"
        case .copyExact: return "Time [\(shortcuts[action].displayText)]"
        case .copyRounded: return "Rounded [\(shortcuts[action].displayText)]"
        default: return action.title
        }
    }
}
