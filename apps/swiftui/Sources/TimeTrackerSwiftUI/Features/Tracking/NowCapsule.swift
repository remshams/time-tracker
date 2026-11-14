import SwiftUI
import TrackerClient

/// The toolbar's one start or stop control, drawn as a plain glass pill like its neighbors. Only the leading dot
/// carries the task's color. Hovering turns the dot into the glyph the click will act as. While a task runs, a click
/// anywhere stops it. While nothing runs, a click starts the selected task.
struct NowCapsule: View {
    let availability: TrackingAvailability
    @Environment(TrackingStore.self) private var tracking
    @Environment(TaskCatalogStore.self) private var catalog
    @Environment(ConnectionStore.self) private var connection

    var body: some View {
        if let active = tracking.active {
            running(active)
        } else if let task = catalog.selectedTask, !task.archived {
            idle(task)
        } else {
            nothingRunning
        }
    }

    private let pillHeight: CGFloat = 34

    private func running(_ active: WorklogItem) -> some View {
        let task = catalog.tasks.first { $0.id == active.taskId }
        let name = task?.name ?? "Running"
        return Button {
            tracking.stopTracking(worklogID: active.id)
        } label: {
            PillLabel(
                color: TaskColor.forTaskID(active.taskId).swiftUIColor, stale: connection.stale,
                glyph: "stop.fill", isEnabled: availability.canStop
            ) {
                Text(name)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 220, alignment: .leading)
                    .fixedSize(horizontal: true, vertical: false)
                TrackerElapsedText()
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .accessibilityHidden(true)
            }
        }
        .buttonStyle(PillButtonStyle(height: pillHeight))
        .disabled(!availability.canStop)
        .contextMenu {
            Button("Show Task") {
                if task?.archived == false { catalog.changeTab(.active) }
                catalog.select(active.taskId)
            }
        }
        .help(connection.stale ? "Stop tracking \(name). The connection is stale." : "Stop tracking \(name)")
        .accessibilityLabel("Stop tracking")
        .accessibilityValue(name)
        .accessibilityChildren { TrackerElapsedText() }
    }

    private func idle(_ task: TaskItem) -> some View {
        let canStart = availability.canStart(taskID: task.id)
        return Button {
            tracking.startTracking(taskID: task.id)
        } label: {
            PillLabel(
                color: TaskColor.forTaskID(task.id).swiftUIColor, stale: connection.stale,
                glyph: "play.fill", isEnabled: canStart
            ) {
                Text("Start").foregroundStyle(.secondary)
                Text(task.name)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 220, alignment: .leading)
                    .fixedSize(horizontal: true, vertical: false)
            }
        }
        .buttonStyle(PillButtonStyle(height: pillHeight))
        .disabled(!canStart)
        .help(connection.stale ? "Start tracking \(task.name). The connection is stale." : "Start tracking \(task.name)")
        .accessibilityLabel("Start tracking")
        .accessibilityValue(task.name)
    }

    private var nothingRunning: some View {
        HStack(spacing: 8) {
            Circle().strokeBorder(.tertiary, lineWidth: 1.5).frame(width: 8, height: 8)
                .frame(width: PillLabel<EmptyView>.leadingSlot)
            Text("Nothing running").foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .frame(height: pillHeight)
        .glassEffect(.regular, in: .capsule)
    }
}

/// The pill's content: a leading slot that holds the dot, or the action glyph while the pointer is over an enabled
/// control, then the caller's text.
private struct PillLabel<Content: View>: View {
    static var leadingSlot: CGFloat { 14 }

    let color: Color
    let stale: Bool
    let glyph: String
    let isEnabled: Bool
    @ViewBuilder let content: Content
    @Environment(\.isHovering) private var isHovering

    var body: some View {
        let showsGlyph = isHovering && isEnabled
        let tint = stale ? Color.secondary : color
        HStack(spacing: 8) {
            ZStack {
                Circle().fill(tint).frame(width: 8, height: 8)
                    .opacity(showsGlyph ? 0 : 1).scaleEffect(showsGlyph ? 0.4 : 1)
                Image(systemName: glyph)
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(tint)
                    .opacity(showsGlyph ? 1 : 0).scaleEffect(showsGlyph ? 1 : 0.4)
            }
            .frame(width: Self.leadingSlot, height: Self.leadingSlot)
            .animation(.easeOut(duration: 0.12), value: showsGlyph)
            content
        }
    }
}

private struct HoveringKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    fileprivate var isHovering: Bool {
        get { self[HoveringKey.self] }
        set { self[HoveringKey.self] = newValue }
    }
}

/// An untinted glass capsule at toolbar control height. The glass reacts to the pointer, and a faint wash marks the
/// press.
private struct PillButtonStyle: ButtonStyle {
    let height: CGFloat
    @State private var isHovering = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .environment(\.isHovering, isHovering)
            .padding(.horizontal, 12)
            .frame(height: height)
            .glassEffect(.regular.interactive(), in: .capsule)
            .overlay { Capsule().fill(Color.primary.opacity(configuration.isPressed ? 0.1 : 0)) }
            .contentShape(Capsule())
            .onHover { isHovering = $0 }
    }
}
