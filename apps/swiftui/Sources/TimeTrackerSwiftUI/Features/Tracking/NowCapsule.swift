import SwiftUI
import TrackerClient

/// The toolbar's one start or stop control, a glass pill that reads like a toolbar item labelled with what you are
/// doing: marker, task name, then the time while a task runs or the verb while it doesn't. A click anywhere on it stops
/// the running task, and for a few seconds afterwards the pill offers to resume it. While nothing runs, a click starts
/// the selected task. The marker is a task-colored dot while running, a hollow ring while idle and a `wifi.slash` glyph
/// while the connection is stale. Hovering turns it into a disc with the glyph the click will act as and tints the
/// glass in the task color.
///
/// Running, resume and start are one button whose content follows a `PillState`, so a click never rebuilds the view and
/// the hover survives it. While a request is in flight the pill keeps showing the state the click acted on and then
/// switches to the new state in one step. The trailing slot reserves the width of the widest time or verb, so the pill
/// keeps its width when running turns into resume or start.
struct NowCapsule: View {
    let availability: TrackingAvailability
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(ConnectionStore.self) private var connection
    @State private var justStopped: StoppedTask?
    @State private var isHovering = false
    @State private var frozen: PillState?
    @State private var pendingStopName: String?

    private static let resumeWindow: Duration = .seconds(6)
    private let pillHeight: CGFloat = 34

    var body: some View {
        Group {
            if let pill = shown {
                button(pill)
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
        .onChange(of: tracking.isSubmitting) { _, submitting in
            if submitting {
                frozen = current
            } else {
                frozen = nil
                finishRequest()
            }
        }
    }

    private var stale: Bool { connection.stale }

    /// What the store says the pill should show right now.
    private var current: PillState? {
        if let active = tracking.active {
            let task = catalog.tasks.first { $0.id == active.taskId }
            return PillState(
                mode: .running(worklogID: active.id), taskID: active.taskId, name: task?.name ?? "Running",
                archived: task?.archived == true)
        }
        if let stopped = justStopped, let task = catalog.tasks.first(where: { $0.id == stopped.taskID }),
            !task.archived
        {
            return PillState(mode: .resume, taskID: task.id, name: task.name, archived: false)
        }
        if let task = catalog.selectedTask, !task.archived {
            return PillState(mode: .start, taskID: task.id, name: task.name, archived: false)
        }
        return nil
    }

    /// The state the click acted on while its request is in flight, so the pill doesn't flicker through the
    /// intermediate states of the reload.
    private var shown: PillState? { tracking.isSubmitting ? (frozen ?? current) : current }

    private func button(_ pill: PillState) -> some View {
        let enabled =
            switch pill.mode {
            case .running: availability.canStop
            case .resume, .start: availability.canStart(taskID: pill.taskID)
            }
        return Button {
            perform(pill)
        } label: {
            PillLabel(pill: pill, stale: stale, inFlight: tracking.isSubmitting)
        }
        // The disabled state keeps a request in flight from sending a second one. The style doesn't dim for it.
        .buttonStyle(
            PillButtonStyle(
                height: pillHeight, color: pill.color, restingTint: pill.restingTint, stale: stale,
                inFlight: tracking.isSubmitting, isHovering: isHovering)
        )
        .disabled(!enabled)
        .onHover { isHovering = $0 }
        .contextMenu { Button("Show Task") { showTask(pill) } }
        .help(stale ? "\(pill.help). The connection is stale." : pill.help)
        .accessibilityLabel(pill.accessibilityLabel)
        // The UI tests look the button up as "Start tracking" after a stop, so the resume state keeps that identifier.
        .accessibilityIdentifier(pill.accessibilityIdentifier)
        .accessibilityValue(
            spokenValue(pill.name, duration: pill.isRunning ? readableDuration(clockText: tracking.timerText) : nil)
        )
        .accessibilityAction(named: "Show Task") { showTask(pill) }
        .accessibilityChildren {
            if pill.isRunning { TrackerElapsedText() }
        }
    }

    private func perform(_ pill: PillState) {
        switch pill.mode {
        case .running(let worklogID):
            tracking.stopTracking(worklogID: worklogID)
            guard tracking.isSubmitting else { return }
            justStopped = StoppedTask(taskID: pill.taskID)
            pendingStopName = pill.name
        case .resume, .start:
            tracking.startTracking(taskID: pill.taskID)
        }
    }

    /// Runs when a request ends. The stop is announced only if it worked, and a stop that failed takes back the offer to
    /// resume.
    private func finishRequest() {
        if tracking.active != nil { justStopped = nil }
        guard let name = pendingStopName else { return }
        pendingStopName = nil
        if tracking.active == nil, tracking.error == nil {
            AccessibilityNotification.Announcement("Stopped \(name). Resume available.").post()
        }
    }

    private func spokenValue(_ name: String, duration: String? = nil) -> String {
        [name, duration, stale ? "connection stale" : nil].compactMap { $0 }.joined(separator: ", ")
    }

    private var nothingRunning: some View {
        HStack(spacing: 8) {
            Circle().strokeBorder(.tertiary, lineWidth: 1.5).frame(width: 9, height: 9)
                .frame(width: PillLabel.markerSlot)
            Text("Nothing running").font(.system(size: 13)).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 14)
        .frame(height: pillHeight)
        .glassEffect(.regular, in: .capsule)
    }

    private func showTask(_ pill: PillState) {
        if !pill.archived { catalog.changeTab(.active) }
        catalog.select(pill.taskID)
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

/// Everything the pill shows and does for one task, so one button can render running, resume and start.
private struct PillState: Equatable {
    enum Mode: Equatable {
        case running(worklogID: String)
        case resume
        case start
    }

    let mode: Mode
    let taskID: String
    let name: String
    let archived: Bool

    var isRunning: Bool {
        if case .running = mode { return true }
        return false
    }

    var color: Color { TaskColor.forTaskID(taskID).swiftUIColor }
    var marker: PillMarker { isRunning ? .running : .idle }
    var glyph: String { marker.glyph }
    var restingTint: Double { mode == .resume ? 0.1 : 0 }

    /// The trailing word of an idle pill, or `nil` while the time shows.
    var verb: String? {
        switch mode {
        case .running: nil
        case .resume: "Resume"
        case .start: "Start"
        }
    }

    var help: String {
        switch mode {
        case .running: "Stop tracking \(name)"
        case .resume: "Resume tracking \(name)"
        case .start: "Start tracking \(name)"
        }
    }

    var accessibilityLabel: String {
        switch mode {
        case .running: "Stop tracking"
        case .resume: "Resume tracking"
        case .start: "Start tracking"
        }
    }

    var accessibilityIdentifier: String { isRunning ? "Stop tracking" : "Start tracking" }
}

/// The pill's content, name first: the marker, the task name, then the time or verb. The marker is a dot while running,
/// a hollow ring while idle and a `wifi.slash` glyph while the connection is stale. It becomes a filled disc with the
/// action glyph while the pointer is over a control that can be clicked, or whose click is still in flight.
private struct PillLabel: View {
    static var markerSlot: CGFloat { 18 }

    let pill: PillState
    let stale: Bool
    let inFlight: Bool
    @Environment(\.isHovering) private var isHovering
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.isPressing) private var isPressing
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        let showsGlyph = (isHovering || isPressing) && (isEnabled || inFlight)
        let color = pill.color
        // Light colors need the darker disc under the white glyph in dark mode. In light mode darkening turns orange
        // brown, so the disc keeps the task color.
        let disc = colorScheme == .dark ? color.mix(with: .black, by: 0.3) : color
        HStack(spacing: 8) {
            // Every marker stays in the layout and fades by opacity, so a state change is one crossfade and the
            // hover doesn't shift the text.
            ZStack {
                Circle().fill(color).frame(width: 9, height: 9)
                    .opacity(pill.marker == .running && !stale ? 1 : 0)
                Circle().strokeBorder(color, lineWidth: 1.75).frame(width: 10, height: 10)
                    .opacity(pill.marker == .idle && !stale ? 1 : 0)
                Image(systemName: "wifi.slash")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .opacity(stale ? 1 : 0)
                Circle().fill(disc).opacity(showsGlyph ? 1 : 0)
                Image(systemName: pill.glyph)
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(.white)
                    .contentTransition(.symbolEffect(.replace))
                    .opacity(showsGlyph ? 1 : 0)
            }
            .frame(width: Self.markerSlot, height: Self.markerSlot)
            .scaleEffect(isPressing && isEnabled ? 0.92 : 1)
            .animation(.easeOut(duration: 0.15), value: showsGlyph)
            .animation(.easeOut(duration: 0.15), value: pill.marker)
            .animation(.easeOut(duration: 0.15), value: pill.glyph)
            .animation(.easeOut(duration: 0.15), value: stale)
            .animation(.easeOut(duration: 0.12), value: isPressing)
            .padding(.leading, -3)
            Text(pill.name)
                .font(.system(size: 13))
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: 220, alignment: .leading)
                .fixedSize(horizontal: true, vertical: false)
                .contentTransition(.opacity)
            PillTrailing(pill: pill).padding(.leading, 2)
        }
        // Another task's name changes the width, which eases instead of snapping.
        .animation(.easeInOut(duration: 0.2), value: pill.name)
    }
}

/// The time while a task runs and the verb while it doesn't. Hidden copies of the widest time and both verbs reserve the
/// width, so the pill is as wide in one state as in another. The two visible texts fade into each other.
private struct PillTrailing: View {
    let pill: PillState
    // The text of the layer that fades out stays put, so a stop doesn't swap the time for another word mid-fade.
    @State private var lastVerb = "Start"

    var body: some View {
        ZStack(alignment: .leading) {
            Group {
                Text("00:00:00").monospacedDigit()
                Text("Resume")
                Text("Start")
            }
            .hidden()
            PillElapsedText().opacity(pill.isRunning ? 1 : 0)
            Text(pill.verb ?? lastVerb).opacity(pill.isRunning ? 0 : 1)
        }
        .font(.system(size: 13, weight: .medium))
        .foregroundStyle(.primary)
        .fixedSize()
        .animation(.easeOut(duration: 0.18), value: pill.isRunning)
        .onChange(of: pill.verb, initial: true) { _, verb in
            if let verb { lastVerb = verb }
        }
        .accessibilityHidden(true)
    }
}

/// The elapsed time of the running worklog. After a stop it keeps the last second, so the fade-out doesn't show "Idle".
private struct PillElapsedText: View {
    @Environment(TrackingStore.self) private var tracking
    @State private var lastRunning = ""

    var body: some View {
        Text(readableDuration(clockText: tracking.active == nil ? lastRunning : tracking.timerText))
            .monospacedDigit()
            .onChange(of: tracking.timerText, initial: true) { _, text in
                if tracking.active != nil { lastRunning = text }
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
/// A disabled control dims to half and takes no wash, and that beats the stale connection's 0.7. A request in flight
/// blocks the click but keeps the hover wash and the opacity of the state it was sent from, so a click doesn't flash
/// the pill. `NowCapsule` owns the hover, because the style is rebuilt with the button's state and a hover kept here
/// would have to outlive that.
private struct PillButtonStyle: ButtonStyle {
    let height: CGFloat
    let color: Color
    var restingTint: Double = 0
    var stale = false
    var inFlight = false
    var isHovering = false
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        let interactive = isEnabled || inFlight
        let pressed = isEnabled && configuration.isPressed
        let hovering = interactive && isHovering
        let tint = pressed ? 0.28 : hovering ? 0.15 : interactive ? restingTint : 0
        let dim = (!isEnabled && !inFlight) ? 0.5 : stale ? 0.7 : 1
        return configuration.label
            .environment(\.isHovering, isHovering)
            .environment(\.isPressing, configuration.isPressed)
            .padding(.horizontal, 14)
            .frame(height: height)
            .glassEffect(interactive ? .regular.interactive() : .regular, in: .capsule)
            .overlay { Capsule().fill(color.opacity(tint)).allowsHitTesting(false) }
            .opacity(dim)
            .animation(.easeOut(duration: 0.12), value: tint)
            .contentShape(Capsule())
    }
}
