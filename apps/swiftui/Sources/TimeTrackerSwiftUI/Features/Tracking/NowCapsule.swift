import SwiftUI
import TrackerClient

/// The toolbar's one start or stop control. While a task runs, the glass capsule carries its color, name and live
/// time with a stop disc. While nothing runs, it starts the selected task.
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
            Text("Nothing running")
                .foregroundStyle(.secondary)
                .padding(.horizontal, 16)
                .frame(height: capsuleHeight)
                .glassEffect(.regular, in: .capsule)
        }
    }

    private let capsuleHeight: CGFloat = 40

    private func running(_ active: WorklogItem) -> some View {
        let color = TaskColor.forTaskID(active.taskId).swiftUIColor
        let task = catalog.tasks.first { $0.id == active.taskId }
        return HStack(spacing: 12) {
            Button {
                if task?.archived == false { catalog.changeTab(.active) }
                catalog.select(active.taskId)
            } label: {
                HStack(spacing: 9) {
                    ZStack {
                        Circle().stroke(color.opacity(0.35), lineWidth: 3).frame(width: 16, height: 16)
                        Circle().fill(connection.stale ? Color.secondary : color).frame(width: 10, height: 10)
                    }
                    Text(task?.name ?? "Running")
                        .font(.system(size: 14, weight: .semibold))
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: 200, alignment: .leading)
                        .fixedSize(horizontal: true, vertical: false)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Show this task's worklogs")
            .accessibilityLabel("Running task")
            .accessibilityValue(task?.name ?? "Unknown task")

            TrackerElapsedText()
                .font(.system(size: 19, weight: .semibold, design: .rounded))
                .monospacedDigit()
                .fixedSize()

            Button {
                tracking.stopTracking(worklogID: active.id)
            } label: {
                Image(systemName: "stop.fill")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 30, height: 30)
                    .background(color, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .disabled(!availability.canStop)
            .help("Stop tracking this task")
            .accessibilityLabel("Stop tracking")
        }
        .padding(.leading, 16)
        .padding(.trailing, 5)
        .frame(height: capsuleHeight)
        .glassEffect(.regular.tint(color.opacity(0.22)), in: .capsule)
    }

    private func idle(_ task: TaskItem) -> some View {
        let color = TaskColor.forTaskID(task.id).swiftUIColor
        return Button {
            tracking.startTracking(taskID: task.id)
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "play.fill")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 30, height: 30)
                    .background(color, in: Circle())
                Text("Start")
                    .foregroundStyle(.secondary)
                Text(task.name)
                    .fontWeight(.semibold)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 220, alignment: .leading)
                    .fixedSize(horizontal: true, vertical: false)
            }
            .font(.system(size: 14))
            .padding(.leading, 5)
            .padding(.trailing, 18)
            .frame(height: capsuleHeight)
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .glassEffect(.regular.tint(color.opacity(0.14)).interactive(), in: .capsule)
        .disabled(!availability.canStart(taskID: task.id))
        .help(
            tracking.active == nil
                ? "Start tracking this task" : "Stop the current timer and start tracking this task"
        )
        .accessibilityLabel("Start tracking")
        .accessibilityValue(task.name)
    }
}
