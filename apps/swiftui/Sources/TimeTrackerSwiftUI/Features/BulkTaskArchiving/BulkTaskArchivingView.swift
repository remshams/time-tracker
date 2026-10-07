import SwiftUI
import TrackerClient

@MainActor
struct BulkTaskArchivingDialog: View {
    @ObservedObject var archiving: BulkTaskArchivingStore
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear.frame(width: 0, height: 0)
            .sheet(isPresented: Binding(
                get: { isPresentationOwner && archiving.state.isPresented },
                set: { if isPresentationOwner && !$0 { archiving.cancel() } }
            )) {
                BulkTaskArchivingSheet(archiving: archiving)
            }
    }
}

@MainActor
private struct BulkTaskArchivingSheet: View {
    @ObservedObject var archiving: BulkTaskArchivingStore

    var body: some View {
        let state = archiving.sheetContent
        VStack(alignment: .leading, spacing: 16) {
            Text("Archive inactive tasks").font(.title2.bold())
            Text("Archive tasks with no recent work or changes. Running tasks are excluded. Worklog history remains available.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack {
                Text("Inactive for more than")
                TextField("Days", text: Binding(get: { state.daysText }, set: archiving.updateDays))
                    .frame(width: 100)
                    .textFieldStyle(.roundedBorder)
                    .disabled(state.isSubmitting)
                    .accessibilityLabel("Inactivity period in days")
                Text("days")
                Spacer()
                Button("Refresh preview") { archiving.refresh() }
                    .disabled(state.isSubmitting || state.isLoading)
            }
            if state.isLoading {
                HStack { ProgressView().controlSize(.small); Text("Loading eligible tasks...") }
                    .frame(maxWidth: .infinity, minHeight: 150)
            } else if let count = state.archivedCount {
                Label("Archived \(count) \(count == 1 ? "task" : "tasks").", systemImage: "checkmark.circle")
                    .frame(maxWidth: .infinity, minHeight: 150, alignment: .leading)
            } else {
                Text("\(state.tasks.count) \(state.tasks.count == 1 ? "task" : "tasks") to archive")
                    .font(.headline)
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(state.tasks) { task in
                            Text(task.name).frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                            Divider()
                        }
                        if state.tasks.isEmpty, state.error == nil {
                            Text("No tasks match this inactivity period.").foregroundStyle(.secondary)
                        }
                    }
                    .padding(12)
                }
                .frame(height: 220)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            }
            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            HStack(spacing: 12) {
                if state.isSubmitting {
                    ProgressView().controlSize(.small)
                    Text("Archiving tasks...").foregroundStyle(.secondary)
                }
                Spacer()
                Button(state.archivedCount == nil ? "Cancel" : "Done") { archiving.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                if state.archivedCount == nil {
                    Button("Archive all") { archiving.submit() }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent)
                        .disabled(!state.canSubmit)
                }
            }
        }
        .padding(24)
        .frame(width: 560)
        .interactiveDismissDisabled(state.isSubmitting)
    }
}
