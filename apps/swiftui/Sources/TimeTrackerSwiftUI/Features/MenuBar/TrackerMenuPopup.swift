import SwiftUI
import TrackerClient

@MainActor
struct TrackerMenuPopup: View {
    let store: TrackerStore
    let showTracker: () -> Void
    let quit: () -> Void
    let close: () -> Void
    let width: CGFloat
    let maximumHeight: CGFloat
    @ObservedObject private var menu: TrackerMenuStore
    @FocusState private var hasKeyboardFocus: Bool
    @State private var navigation = TrackerMenuNavigation()
    @State private var otherTasksExpanded = false

    init(store: TrackerStore, showTracker: @escaping () -> Void, quit: @escaping () -> Void,
         close: @escaping () -> Void, width: CGFloat = 360, maximumHeight: CGFloat = 560) {
        self.store = store
        self.showTracker = showTracker
        self.quit = quit
        self.close = close
        self.width = width
        self.maximumHeight = maximumHeight
        menu = store.menu
    }

    var body: some View {
        let content = menu.content
        let actions = availableActions(content)
        let displayedActions = content.displayedActions(otherTasksExpanded: otherTasksExpanded)
        ScrollViewReader { scroll in
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    trackingStatus(content)
                    Divider()
                    today(content)
                    if !content.otherTasks.isEmpty {
                        otherTasks(content)
                    }
                    Divider()
                    HStack {
                        Button("Open Time Tracker") { perform(.openTracker) }
                            .modifier(MenuActionHighlight(action: .openTracker,
                                                          navigation: $navigation))
                        Spacer()
                        Button("Quit") { perform(.quit) }
                            .modifier(MenuActionHighlight(action: .quit,
                                                          navigation: $navigation))
                            .accessibilityLabel("Quit Time Tracker")
                    }
                }
                .padding(16)
            }
            .onChange(of: navigation.selection) { _, selection in
                if let selection { scroll.scrollTo(selection) }
            }
        }
        .frame(width: width)
        .frame(maxHeight: maximumHeight)
        .fixedSize(horizontal: false, vertical: true)
        .focusable()
        .focusEffectDisabled()
        .focused($hasKeyboardFocus)
        .onChange(of: actions, initial: true) { _, actions in
            navigation.update(actions: actions, displayedActions: displayedActions)
        }
        .onChange(of: displayedActions) { _, displayedActions in
            navigation.update(actions: actions, displayedActions: displayedActions)
        }
        .task { hasKeyboardFocus = true }
        .onKeyPress(.upArrow, phases: [.down, .repeat]) { _ in navigate(forward: false) }
        .onKeyPress(.downArrow, phases: [.down, .repeat]) { _ in navigate(forward: true) }
        .onKeyPress(.home) { navigation.selectBoundary(first: true); return .handled }
        .onKeyPress(.end) { navigation.selectBoundary(first: false); return .handled }
        .onKeyPress(.tab, phases: [.down, .repeat]) { press in
            navigate(forward: !press.modifiers.contains(.shift))
        }
        .onKeyPress(.return) { activateSelection() }
        .onKeyPress(.space) { activateSelection() }
        .onKeyPress(.escape) { close(); return .handled }
        .onKeyPress(.rightArrow) {
            guard navigation.selection == .otherTasks else { return .ignored }
            otherTasksExpanded = true
            return .handled
        }
        .onKeyPress(.leftArrow) {
            guard otherTasksExpanded else { return .ignored }
            otherTasksExpanded = false
            navigation.select(.otherTasks)
            return .handled
        }
    }

    private func availableActions(_ content: TrackerMenuContent) -> [TrackerMenuAction] {
        content.availableActions(otherTasksExpanded: otherTasksExpanded)
    }

    private func navigate(forward: Bool) -> KeyPress.Result {
        navigation.move(forward: forward)
        return .handled
    }

    private func activateSelection() -> KeyPress.Result {
        guard let selection = navigation.selection else { return .ignored }
        perform(selection)
        return .handled
    }

    private func perform(_ action: TrackerMenuAction) {
        guard availableActions(menu.content).contains(action) else { return }
        switch action {
        case .stop(let id):
            store.stopTracking(worklogID: id)
            close()
        case .task(let id):
            store.startTracking(taskID: id)
            close()
        case .otherTasks:
            otherTasksExpanded.toggle()
        case .openTracker:
            close()
            showTracker()
        case .quit:
            close()
            quit()
        }
    }

    private func otherTasks(_ content: TrackerMenuContent) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Button { perform(.otherTasks) } label: {
                HStack {
                    Image(systemName: otherTasksExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption)
                    Text("Start tracking")
                    Spacer()
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .modifier(MenuActionHighlight(action: .otherTasks, navigation: $navigation))
            .accessibilityValue(otherTasksExpanded ? "Expanded" : "Collapsed")
            if otherTasksExpanded {
                VStack(spacing: 2) {
                    ForEach(content.otherTasks) { entry in
                        taskButton(entry, content: content, showsDuration: false)
                    }
                }
                .padding(.leading, 12)
            }
        }
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
                    perform(.stop(worklogID: worklogID))
                }
                .modifier(MenuActionHighlight(action: .stop(worklogID: worklogID),
                                              navigation: $navigation))
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
            perform(.task(id: entry.id))
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
        .modifier(MenuActionHighlight(action: .task(id: entry.id), navigation: $navigation))
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

private struct MenuActionHighlight: ViewModifier {
    let action: TrackerMenuAction
    @Binding var navigation: TrackerMenuNavigation

    func body(content: Content) -> some View {
        content
            .focusable(false)
            .padding(4)
            .background(navigation.selection == action ? Color.accentColor.opacity(0.18) : .clear,
                        in: RoundedRectangle(cornerRadius: 5))
            .onHover { hovering in
                if hovering { navigation.select(action) }
            }
            .accessibilityAddTraits(navigation.selection == action ? .isSelected : [])
            .id(action)
    }
}
