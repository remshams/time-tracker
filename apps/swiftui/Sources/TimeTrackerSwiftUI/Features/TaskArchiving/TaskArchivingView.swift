import SwiftUI
import TrackerClient

@MainActor
struct TaskArchivingDialog: View {
    @ObservedObject var archiving: TaskArchivingStore
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(isPresented: Binding(
                get: { isPresentationOwner && archiving.state.isPresented },
                set: { if isPresentationOwner && !$0 { archiving.cancel() } }
            )) {
                TaskArchivingSheet(archiving: archiving)
            }
    }
}

@MainActor
private struct TaskArchivingSheet: View {
    @ObservedObject var archiving: TaskArchivingStore

    var body: some View {
        let state = archiving.sheetContent
        let isUnarchiving = state.action == .unarchive
        let actionTitle = isUnarchiving ? "Unarchive" : "Archive"
        VStack(alignment: .leading, spacing: 16) {
            Text("\(actionTitle) task").font(.title2.bold())
            Text(state.taskName)
                .font(.headline)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Text(isUnarchiving
                 ? "The task will return to Active. Its worklog history will remain available."
                 : "The task will move to Archived. Its worklog history will remain available.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            if state.requiresReview, let latest = state.latest {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Latest task").font(.subheadline.weight(.semibold))
                    Text(latest.name).textSelection(.enabled)
                    Text(latest.archived ? "Archived" : "Active")
                        .foregroundStyle(.secondary)
                    Button("Review latest task") { archiving.reviewLatest() }
                        .disabled(state.isSubmitting)
                }
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            }

            HStack(spacing: 12) {
                if state.isSubmitting {
                    ProgressView().controlSize(.small)
                    Text(isUnarchiving ? "Unarchiving task..." : "Archiving task...")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel") { archiving.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button(state.hasUnresolvedIntent && state.error != nil ? "Retry" : actionTitle) {
                    archiving.submit()
                }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(!state.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 480)
        .interactiveDismissDisabled(state.isSubmitting)
    }
}

@MainActor
struct PendingTaskArchivingButton: View {
    @ObservedObject var archiving: TaskArchivingStore

    var body: some View {
        if archiving.state.hasPendingAction, archiving.state.error != nil {
            Button("Review task action") { archiving.reopen() }
                .disabled(archiving.state.isSubmitting)
        }
    }
}
