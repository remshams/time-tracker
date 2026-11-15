import SwiftUI
import TrackerClient

/// The toolbar's one start and stop control, a glass pill: marker, task name, then the time while a task runs or
/// "Start" while it doesn't. `TrackingPill` decides what it shows and does; this view draws it and owns the hover.
///
/// Running and idle are one button whose content follows the pill's state, so a click never rebuilds the view and the
/// hover survives it. The trailing slot reserves the width of the widest time, so the pill keeps its width when a
/// click switches the state.
struct TrackingPillView: View {
    @Environment(TrackingPill.self) private var pill
    @State private var isHovering = false

    fileprivate static let height: CGFloat = 34

    var body: some View {
        Group {
            if let shown = pill.shown {
                button(shown)
            } else {
                nothingRunning
            }
        }
        // The hover lives here, so a pointer that leaves while "Nothing running" shows still ends it.
        .onHover { isHovering = $0 }
    }

    private func button(_ shown: TrackingPillState) -> some View {
        Button {
            pill.click()
        } label: {
            TrackingPillLabel(state: shown, stale: pill.isStale, inFlight: pill.isInFlight)
        }
        // The disabled state keeps a request in flight from sending a second one. The style doesn't dim for it.
        .buttonStyle(
            TrackingPillButtonStyle(
                color: TaskColor.forTaskID(shown.taskID), inFlight: pill.isInFlight, isHovering: isHovering)
        )
        .disabled(!pill.isEnabled)
        .contextMenu { Button("Show Task") { pill.showTask() } }
        .help(shown.help(stale: pill.isStale))
        .accessibilityLabel(shown.actionLabel)
        .accessibilityValue(shown.accessibilityValue(stale: pill.isStale))
        .accessibilityAction(named: "Show Task") { pill.showTask() }
    }

    private var nothingRunning: some View {
        HStack(spacing: Spacing.small) {
            Circle().strokeBorder(.tertiary, lineWidth: BorderWidth.marker)
                .frame(width: IconSize.marker, height: IconSize.marker)
                .frame(width: IconSize.regular)
            Text("Nothing running").font(TextRole.status).foregroundStyle(.secondary)
        }
        .padding(.horizontal, Spacing.medium)
        .frame(height: Self.height)
        .glassEffect(.regular, in: .capsule)
        .accessibilityElement(children: .combine)
    }
}

/// The marker, the task name, then the time or "Start". The marker is a dot while running, a hollow ring while idle and
/// `wifi.slash` while the connection is stale. It becomes a filled disc with the action glyph while the pointer is over
/// a control that can be clicked, or whose click is still in flight.
private struct TrackingPillLabel: View {
    let state: TrackingPillState
    let stale: Bool
    let inFlight: Bool
    @Environment(\.trackingPillHovering) private var isHovering
    @Environment(\.trackingPillPressing) private var isPressing
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.colorScheme) private var colorScheme

    private static let nameWidth: CGFloat = 220

    var body: some View {
        let taskColor = TaskColor.forTaskID(state.taskID)
        let color = ColorRole.task(taskColor)
        let showsGlyph = (isHovering || isPressing) && (isEnabled || inFlight)
        HStack(spacing: Spacing.small) {
            // Every marker stays in the layout and fades by opacity, so a state change is one crossfade and the hover
            // doesn't shift the text.
            ZStack {
                Circle().fill(color).frame(width: IconSize.marker, height: IconSize.marker)
                    .opacity(state.isRunning && !stale ? 1 : 0)
                Circle().strokeBorder(color, lineWidth: BorderWidth.marker)
                    .frame(width: IconSize.marker, height: IconSize.marker)
                    .opacity(!state.isRunning && !stale ? 1 : 0)
                Image(systemName: "wifi.slash")
                    .font(TextRole.statusGlyph)
                    .foregroundStyle(.secondary)
                    .opacity(stale ? 1 : 0)
                Circle().fill(ColorRole.actionDisc(taskColor, in: colorScheme)).opacity(showsGlyph ? 1 : 0)
                Image(systemName: state.isRunning ? "stop.fill" : "play.fill")
                    .font(TextRole.actionGlyph)
                    .foregroundStyle(.white)
                    .contentTransition(.symbolEffect(.replace))
                    .opacity(showsGlyph ? 1 : 0)
            }
            .frame(width: IconSize.regular, height: IconSize.regular)
            .scaleEffect(isPressing && isEnabled ? Motion.pressScale : 1)
            .animation(Motion.standard, value: showsGlyph)
            .animation(Motion.standard, value: state.isRunning)
            .animation(Motion.standard, value: stale)
            .animation(Motion.quick, value: isPressing)
            Text(state.name)
                .font(TextRole.status)
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.tail)
                .frame(maxWidth: Self.nameWidth, alignment: .leading)
                .fixedSize(horizontal: true, vertical: false)
                .contentTransition(.opacity)
            TrackingPillTrailing(state: state)
        }
        // Another task's name changes the width, which eases instead of snapping.
        .animation(Motion.standard, value: state.name)
    }
}

/// The time while a task runs and "Start" while it doesn't. A hidden copy of the widest time reserves the width, so the
/// pill is as wide in one state as in the other. The seconds are dimmed.
private struct TrackingPillTrailing: View {
    let state: TrackingPillState
    // The time of the layer that fades out stays put, so a stop doesn't blank the time mid-fade.
    @State private var lastElapsed = DurationText(seconds: 0)

    var body: some View {
        let elapsed = state.elapsed ?? lastElapsed
        ZStack(alignment: .leading) {
            Text("00:00:00").hidden()
            Text("\(elapsed.hoursAndMinutes)\(Text(elapsed.seconds).foregroundStyle(.secondary))")
                .opacity(state.isRunning ? 1 : 0)
            Text("Start").opacity(state.isRunning ? 0 : 1)
        }
        .font(TextRole.duration(.body).weight(.medium))
        .foregroundStyle(.primary)
        .fixedSize()
        .animation(Motion.standard, value: state.isRunning)
        .onChange(of: state.elapsed, initial: true) { _, elapsed in
            if let elapsed { lastElapsed = elapsed }
        }
        .accessibilityHidden(true)
    }
}

extension EnvironmentValues {
    @Entry fileprivate var trackingPillHovering = false
    @Entry fileprivate var trackingPillPressing = false
}

/// A glass capsule at toolbar control height. Hovering washes it faintly in the task color and pressing deepens it. A
/// disabled control dims and takes no wash. A request in flight blocks the click but keeps the hover wash and full
/// opacity, so a click doesn't flash the pill. `TrackingPillView` owns the hover, because the style is rebuilt with
/// the button's state and a hover kept here would have to outlive that.
private struct TrackingPillButtonStyle: ButtonStyle {
    let color: TaskColor
    let inFlight: Bool
    let isHovering: Bool
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        let interactive = isEnabled || inFlight
        let pressed = isEnabled && configuration.isPressed
        let tint = pressed ? Emphasis.pressedTint : interactive && isHovering ? Emphasis.hoverTint : 0
        return configuration.label
            .environment(\.trackingPillHovering, isHovering)
            .environment(\.trackingPillPressing, configuration.isPressed)
            .padding(.horizontal, Spacing.medium)
            .frame(height: TrackingPillView.height)
            .glassEffect(interactive ? .regular.interactive() : .regular, in: .capsule)
            .overlay { Capsule().fill(ColorRole.task(color).opacity(tint)).allowsHitTesting(false) }
            .opacity(interactive ? 1 : Emphasis.disabled)
            .animation(Motion.quick, value: tint)
            .contentShape(Capsule())
    }
}
