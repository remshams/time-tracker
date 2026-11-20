import SwiftUI
import TrackerClient

struct BulkTaskArchivingSheet: View {
    @Environment(BulkTaskArchivingStore.self) private var archiving

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.large) {
            Text("Archive inactive tasks").font(TextRole.sheetTitle)
            Text(
                "Archive tasks with no recent work or changes. Running tasks are excluded. Worklog history remains available."
            )
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            HStack {
                Text("Inactive for more than")
                TextField("Days", text: Binding(get: { archiving.daysText }, set: { archiving.setDays($0) }))
                    .frame(width: 100)
                    .textFieldStyle(.roundedBorder)
                    .disabled(archiving.isSubmitting)
                    .accessibilityLabel("Inactivity period in days")
                    .accessibilityIdentifier("bulk-archive.days")
                Text("days")
                Spacer()
                Button {
                    archiving.refresh()
                } label: {
                    DelayedProgress(isBusy: archiving.isLoading) { visible in
                        ZStack {
                            Image(systemName: "arrow.clockwise")
                                .opacity(visible ? 0 : 1)
                            ProgressView().controlSize(.small)
                                .opacity(visible ? 1 : 0)
                                .accessibilityHidden(!visible)
                                .accessibilityIdentifier("bulk-archive.refresh-progress")
                        }
                    }
                    .frame(width: IconSize.regular, height: IconSize.regular)
                }
                .buttonStyle(IconButtonStyle(diameter: IconSize.button))
                .disabled(archiving.isSubmitting || archiving.isLoading)
                .help("Refresh preview")
                .accessibilityLabel(archiving.isLoading ? "Refreshing preview" : "Refresh preview")
                .accessibilityIdentifier("bulk-archive.refresh")
            }
            Text(previewTitle)
                .font(TextRole.heading)
                .frame(maxWidth: .infinity, alignment: .leading)
                .frame(height: 20)
                .accessibilityIdentifier("bulk-archive.count")
            Group {
                if let error = archiving.error {
                    Label(error, systemImage: "exclamationmark.triangle")
                        .foregroundStyle(ColorRole.error)
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                        .accessibilityIdentifier("bulk-archive.error")
                        .padding(Spacing.medium)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: Spacing.small) {
                            if let count = archiving.archivedCount {
                                Label(
                                    "Archived \(count) \(count == 1 ? "task" : "tasks").",
                                    systemImage: "checkmark.circle"
                                )
                                .accessibilityIdentifier("bulk-archive.result")
                            } else {
                                ForEach(archiving.tasks) { task in
                                    Text(task.name).frame(maxWidth: .infinity, alignment: .leading)
                                        .textSelection(.enabled)
                                        .accessibilityIdentifier("bulk-archive.candidate.\(task.id)")
                                    Divider()
                                }
                                if archiving.tasks.isEmpty, archiving.hasPreview {
                                    Text("No tasks match this inactivity period.").foregroundStyle(.secondary)
                                        .accessibilityIdentifier("bulk-archive.empty")
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(Spacing.medium)
                    }
                }
            }
            .frame(height: 220)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: CornerRadius.medium))
            HStack(spacing: Spacing.medium) {
                DelayedProgressIndicator(
                    isBusy: archiving.isSubmitting, title: "Archiving tasks...",
                    identifier: "bulk-archive.submit-progress")
                Spacer()
                Button(archiving.archivedCount == nil ? "Cancel" : "Done") { archiving.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(archiving.isSubmitting)
                    .accessibilityIdentifier("bulk-archive.cancel")
                if archiving.archivedCount == nil {
                    Button("Archive all") { archiving.submit() }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent)
                        .disabled(!archiving.canSubmit)
                        .accessibilityIdentifier("bulk-archive.confirm")
                }
            }
            .frame(height: 24)
        }
        .padding(Spacing.xLarge)
        .frame(width: 560)
        .interactiveDismissDisabled(archiving.isSubmitting)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("bulk-archive.dialog")
    }

    private var previewTitle: String {
        if archiving.archivedCount != nil { return "Archive complete" }
        guard archiving.hasPreview else { return "Eligible tasks" }
        return "\(archiving.tasks.count) \(archiving.tasks.count == 1 ? "task" : "tasks") to archive"
    }
}
