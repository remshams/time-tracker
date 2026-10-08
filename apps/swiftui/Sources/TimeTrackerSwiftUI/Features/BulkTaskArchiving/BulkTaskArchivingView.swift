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
                    .accessibilityIdentifier("bulk-archive.days")
                Text("days")
                Spacer()
                Button { archiving.refresh() } label: {
                    ZStack {
                        Image(systemName: "arrow.clockwise")
                            .opacity(state.isLoading ? 0 : 1)
                        if state.isLoading {
                            ProgressView().controlSize(.small)
                        }
                    }
                    .frame(width: 16, height: 16)
                    .padding(4)
                    .contentShape(Rectangle())
                }
                    .buttonStyle(.plain)
                    .disabled(state.isSubmitting || state.isLoading)
                    .help("Refresh preview")
                    .accessibilityLabel(state.isLoading ? "Refreshing preview" : "Refresh preview")
                    .accessibilityIdentifier("bulk-archive.refresh")
            }
            Text(previewTitle(state))
                .font(.headline)
                .frame(maxWidth: .infinity, alignment: .leading)
                .frame(height: 20)
                .accessibilityIdentifier("bulk-archive.count")
            Group {
                if let error = state.error {
                    Label(error, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                        .accessibilityIdentifier("bulk-archive.error")
                        .padding(12)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 8) {
                            if let count = state.archivedCount {
                                Label("Archived \(count) \(count == 1 ? "task" : "tasks").", systemImage: "checkmark.circle")
                                    .accessibilityIdentifier("bulk-archive.result")
                            } else {
                                ForEach(state.tasks) { task in
                                    Text(task.name).frame(maxWidth: .infinity, alignment: .leading)
                                        .textSelection(.enabled)
                                        .accessibilityIdentifier("bulk-archive.candidate.\(task.id)")
                                    Divider()
                                }
                                if state.tasks.isEmpty, state.hasPreview {
                                    Text("No tasks match this inactivity period.").foregroundStyle(.secondary)
                                        .accessibilityIdentifier("bulk-archive.empty")
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(12)
                    }
                }
            }
            .frame(height: 220)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            HStack(spacing: 12) {
                if state.isSubmitting {
                    ProgressView().controlSize(.small)
                    Text("Archiving tasks...").foregroundStyle(.secondary)
                }
                Spacer()
                Button(state.archivedCount == nil ? "Cancel" : "Done") { archiving.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                    .accessibilityIdentifier("bulk-archive.cancel")
                if state.archivedCount == nil {
                    Button("Archive all") { archiving.submit() }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent)
                        .disabled(!state.canSubmit)
                        .accessibilityIdentifier("bulk-archive.confirm")
                }
            }
            .frame(height: 24)
        }
        .padding(24)
        .frame(width: 560)
        .interactiveDismissDisabled(state.isSubmitting)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("bulk-archive.dialog")
    }

    private func previewTitle(_ state: BulkTaskArchivingPresentation) -> String {
        if state.archivedCount != nil { return "Archive complete" }
        guard state.hasPreview else { return "Eligible tasks" }
        return "\(state.tasks.count) \(state.tasks.count == 1 ? "task" : "tasks") to archive"
    }
}
