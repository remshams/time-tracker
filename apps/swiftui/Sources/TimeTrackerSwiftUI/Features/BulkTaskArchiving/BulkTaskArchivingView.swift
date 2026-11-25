import SwiftUI
import TrackerClient

struct BulkTaskArchivingSheet: View {
    @Environment(BulkTaskArchivingStore.self) private var archiving

    var body: some View {
        DialogSheet(
            title: "Archive inactive tasks", message: "Archive tasks with no recent work or changes.",
            systemImage: "archivebox", width: DialogSize.wideWidth, isBusy: archiving.isSubmitting
        ) {
            DialogSection {
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
                    RefreshPreviewButton()
                }
            } footer: {
                Text("Running tasks are excluded. Worklog history remains available.")
            }

            VStack(alignment: .leading, spacing: Spacing.small) {
                Text(previewTitle)
                    .font(TextRole.heading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, Spacing.medium)
                    .accessibilityIdentifier("bulk-archive.count")
                BulkArchivePreview()
                    .frame(height: DialogSize.listHeight)
                    .dialogGroup()
            }
        } status: {
            DelayedProgressIndicator(
                isBusy: archiving.isSubmitting, title: "Archiving tasks...",
                identifier: "bulk-archive.submit-progress")
        } actions: {
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
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("bulk-archive.dialog")
    }

    private var previewTitle: String {
        if archiving.archivedCount != nil { return "Archive complete" }
        guard archiving.hasPreview else { return "Eligible tasks" }
        return "\(archiving.tasks.count) \(archiving.tasks.count == 1 ? "task" : "tasks") to archive"
    }
}

/// Reloads the preview. Its glyph turns into a spinner once a refresh takes a moment, in the same place, so the row
/// doesn't move.
private struct RefreshPreviewButton: View {
    @Environment(BulkTaskArchivingStore.self) private var archiving

    var body: some View {
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
        .buttonStyle(.icon(.compact))
        .disabled(archiving.isSubmitting || archiving.isLoading)
        .help("Refresh preview")
        .accessibilityLabel(archiving.isLoading ? "Refreshing preview" : "Refresh preview")
        .accessibilityIdentifier("bulk-archive.refresh")
    }
}

/// The tasks the archive would move, or the error, the empty state or the result in the same place, so the sheet
/// keeps its size.
private struct BulkArchivePreview: View {
    @Environment(BulkTaskArchivingStore.self) private var archiving

    var body: some View {
        if let error = archiving.error {
            Label(error, systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(ColorRole.error)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
                .accessibilityIdentifier("bulk-archive.error")
                .padding(Spacing.medium)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let count = archiving.archivedCount {
            Label("Archived \(count) \(count == 1 ? "task" : "tasks").", systemImage: "checkmark.circle")
                .accessibilityIdentifier("bulk-archive.result")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if archiving.tasks.isEmpty, archiving.hasPreview {
            Text("No tasks match this inactivity period.")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("bulk-archive.empty")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(archiving.tasks) { task in
                        if task.id != archiving.tasks.first?.id {
                            Divider()
                        }
                        HStack(spacing: Spacing.small) {
                            // Inactive tasks neither run nor have time today.
                            TaskIndicatorView(taskID: task.id, isRunning: false, hasTimeToday: false)
                            Text(task.name)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                                .accessibilityIdentifier("bulk-archive.candidate.\(task.id)")
                        }
                        .padding(.vertical, Spacing.small)
                    }
                }
                .padding(.horizontal, Spacing.medium)
            }
        }
    }
}
