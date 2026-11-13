import SwiftUI
import TrackerClient

/// The window with no tasks at all: one sentence and the one action that moves on.
struct EmptyTasksView: View {
    @Environment(TaskCreationStore.self) private var creation

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "timer")
                .font(.system(size: 44, weight: .light))
                .foregroundStyle(.tertiary)
            Text("No tasks yet")
                .font(.title2.weight(.semibold))
            Text("Create a task and start its timer. Today's split and the worklogs appear here.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 340)
            Button {
                creation.open()
            } label: {
                Label("Create your first task", systemImage: "plus")
                    .font(.title3.weight(.semibold))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
            }
            .buttonStyle(.glassProminent)
            .buttonBorderShape(.capsule)
            .controlSize(.large)
            .disabled(!creation.canOpen)
            .padding(.top, 6)
            .accessibilityIdentifier("task-details.create")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }
}
