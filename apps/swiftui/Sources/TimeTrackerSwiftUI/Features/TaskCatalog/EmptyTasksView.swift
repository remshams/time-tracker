import SwiftUI
import TrackerClient

/// The detail pane while the tracker has no tasks at all: one sentence and the button that creates the first task.
struct EmptyTasksView: View {
    @Environment(TaskCreationStore.self) private var creation

    var body: some View {
        VStack(spacing: Spacing.medium) {
            Image(systemName: "timer")
                .font(TextRole.display)
                .foregroundStyle(.tertiary)
                .accessibilityHidden(true)
            Text("Create a task to start tracking your time.")
                .font(TextRole.placeholder)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button {
                creation.open()
            } label: {
                Label("Create your first task", systemImage: "plus")
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(!creation.canOpen)
            .help(creation.canOpen ? "Create a task" : "Tasks can't be created right now")
            .accessibilityIdentifier("task-details.create")
        }
        .padding(Spacing.xLarge)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("task-details.empty")
    }
}
