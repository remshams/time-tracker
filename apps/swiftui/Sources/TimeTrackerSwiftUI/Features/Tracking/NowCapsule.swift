import SwiftUI
import TrackerClient

/// The toolbar's one start or stop control, a glass pill that reads like a toolbar item labelled with what you are
/// doing: marker, task name, then the time. While a task runs, a click anywhere on it stops the task, and for a few
/// seconds afterwards the pill offers to resume it. While nothing runs, a click starts the selected task. The marker is
/// a task-colored dot while running and a hollow ring while idle. Hovering turns it into a disc with the glyph the click
/// will act as and tints the glass in the task color.
struct NowCapsule: View {
    let availability: TrackingAvailability
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(ConnectionStore.self) private var connection
    @State private var justStopped: StoppedTask?

    private static let resumeWindow: Duration = .seconds(6)
    private let pillHeight: CGFloat = 34

    var body: some View {
        Group {
            if let active = tracking.active {
                running(active)
            } else if let stopped = justStopped, let task = catalog.tasks.first(where: { $0.id == stopped.taskID }),
                !task.archived
            {
                idle(task, resuming: true)
            } else if let task = catalog.selectedTask, !task.archived {
                idle(task, resuming: false)
            } else {
                nothingRunning
            }
        }
        .task(id: justStopped?.token) {
            guard justStopped != nil else { return }
            try? await Task.sleep(for: Self.resumeWindow)
            justStopped = nil
        }
        .onChange(of: tracking.active?.id) { _, id in
            if id != nil { justStopped = nil }
        }
    }

    private var stale: Bool { connection.stale }

    private func running(_ active: WorklogItem) -> some View {
        let task = catalog.tasks.first { $0.id == active.taskId }
        let name = task?.name ?? "Running"
        return Button {
            justStopped = StoppedTask(taskID: active.taskId)
            tracking.stopTracking(worklogID: active.id)
        } label: {
            PillLabel(
                name: name, marker: .running, color: TaskColor.forTaskID(active.taskId).swiftUIColor, stale: stale
            ) {
                TrackerElapsedText()
                    .font(.system(size: 13, weight: .medium))
                    .monospacedDigit()
                    .foregroundStyle(.primary)
                    .fixedSize()
                    .accessibilityHidden(true)
            }
        }
        .buttonStyle(PillButtonStyle(height: pillHeight, color: TaskColor.forTaskID(active.taskId).swiftUIColor))
        .disabled(!availability.canStop)
        .contextMenu {
            Button("Show Task") { showTask(active.taskId, archived: task?.archived == true) }
        }
        .help(stale ? "Stop tracking \(name). The connection is stale." : "Stop tracking \(name)")
        .accessibilityLabel("Stop tracking")
        .accessibilityValue("\(name), \(readableDuration(clockText: tracking.timerText))")
        .accessibilityAction(named: "Show Task") { showTask(active.taskId, archived: task?.archived == true) }
        .accessibilityChildren { TrackerElapsedText() }
    }

    private func idle(_ task: TaskItem, resuming: Bool) -> some View {
        let color = TaskColor.forTaskID(task.id).swiftUIColor
        let verb = resuming ? "Resume" : "Start"
        let help = "\(verb) tracking \(task.name)"
        return Button {
            justStopped = nil
            tracking.startTracking(taskID: task.id)
        } label: {
            PillLabel(name: task.name, marker: .idle, color: color, stale: stale) {
                Text(verb)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(resuming ? .primary : .secondary)
                    .fixedSize()
                    .accessibilityHidden(true)
            }
        }
        .buttonStyle(PillButtonStyle(height: pillHeight, color: color, restingTint: resuming ? 0.1 : 0))
        .disabled(!availability.canStart(taskID: task.id))
        .help(stale ? "\(help). The connection is stale." : help)
        .accessibilityLabel("Start tracking")
        .accessibilityValue(task.name)
    }

    private var nothingRunning: some View {
        HStack(spacing: 8) {
            Circle().strokeBorder(.tertiary, lineWidth: 1.5).frame(width: 9, height: 9)
                .frame(width: PillLabel<EmptyView>.markerSlot)
            Text("Nothing running").font(.system(size: 13)).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 14)
        .frame(height: pillHeight)
        .glassEffect(.regular, in: .capsule)
    }

    private func showTask(_ id: String, archived: Bool) {
        if !archived { catalog.changeTab(.active) }
        catalog.select(id)
    }
}

/// The task a click on the control just stopped, kept for a few seconds so the pill can offer to resume it.
private struct StoppedTask: Equatable {
    let taskID: String
    let token = UUID()
}

private enum PillMarker {
    case running
    case idle

    var glyph: String { self == .running ? "stop.fill" : "play.fill" }
}

/// The pill's content, name first: the marker, the task name, a hairline, then the caller's trailing text. The marker
/// is a dot while running and a hollow ring while idle. It becomes a filled disc with the action glyph while the
/// pointer is over an enabled control.
private struct PillLabel<Trailing: View>: View {
    static var markerSlot: CGFloat { 16 }

    let name: String
    let marker: PillMarker
    let color: Color
    let stale: Bool
    @ViewBuilder let trailing: Trailing
    @Environment(\.isHovering) private var isHovering
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.isPressing) private var isPressing

    var body: some View {
        let showsGlyph = (isHovering || isPressing) && isEnabled
        let tint = stale ? Color.secondary : color
        HStack(spacing: 8) {
            ZStack {
                switch marker {
                case .running: Circle().fill(tint).frame(width: 9, height: 9)
                case .idle: Circle().strokeBorder(tint, lineWidth: 1.75).frame(width: 10, height: 10)
                }
                // The disc stays in the layout, so the hover doesn't shift the text.
                Circle().fill(tint).opacity(showsGlyph ? 1 : 0)
                Image(systemName: marker.glyph)
                    .font(.system(size: 7, weight: .bold))
                    .foregroundStyle(.white)
                    .opacity(showsGlyph ? 1 : 0)
            }
            .frame(width: Self.markerSlot, height: Self.markerSlot)
            .scaleEffect(isPressing && isEnabled ? 0.92 : 1)
            .animation(.easeOut(duration: 0.12), value: showsGlyph)
            .padding(.trailing, -2)
            Text(name)
                .font(.system(size: 13))
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: 220, alignment: .leading)
                .fixedSize(horizontal: true, vertical: false)
            Rectangle()
                .fill(.primary.opacity(0.22))
                .frame(width: 1, height: 14)
            trailing
        }
    }
}

private struct HoveringKey: EnvironmentKey {
    static let defaultValue = false
}

private struct PressingKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    fileprivate var isHovering: Bool {
        get { self[HoveringKey.self] }
        set { self[HoveringKey.self] = newValue }
    }

    fileprivate var isPressing: Bool {
        get { self[PressingKey.self] }
        set { self[PressingKey.self] = newValue }
    }
}

/// A glass capsule at toolbar control height. Hovering washes it faintly in the task color and pressing deepens it.
/// A disabled control dims and takes no wash.
private struct PillButtonStyle: ButtonStyle {
    let height: CGFloat
    let color: Color
    var restingTint: Double = 0
    @State private var isHovering = false
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        let pressed = isEnabled && (configuration.isPressed)
        let hovering = isEnabled && isHovering
        let tint = pressed ? 0.28 : hovering ? 0.15 : isEnabled ? restingTint : 0
        return configuration.label
            .environment(\.isHovering, isHovering)
            .environment(\.isPressing, configuration.isPressed)
            .padding(.horizontal, 14)
            .frame(height: height)
            .glassEffect(isEnabled ? .regular.interactive() : .regular, in: .capsule)
            .overlay { Capsule().fill(color.opacity(tint)).allowsHitTesting(false) }
            .opacity(isEnabled ? 1 : 0.5)
            .animation(.easeOut(duration: 0.12), value: tint)
            .contentShape(Capsule())
            .onHover { isHovering = $0 }
    }
}
