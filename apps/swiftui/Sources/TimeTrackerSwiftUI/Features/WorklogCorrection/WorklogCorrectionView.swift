import SwiftUI
import TrackerClient

struct WorklogCorrectionSheet: View {
    @Environment(WorklogCorrectionStore.self) private var correction

    var body: some View {
        let taskID = correction.original?.taskId
        DialogSheet(
            title: "Edit worklog", message: correction.taskName, systemImage: "clock", tint: .task(id: taskID),
            width: DialogSize.wideWidth, error: correction.error, errorIdentifier: "worklog-correction.error",
            isBusy: correction.isSubmitting
        ) {
            if let taskID {
                DialogSection {
                    WorklogSpanPanel(
                        start: correction.start, end: correction.end, taskID: taskID,
                        durationIdentifier: "worklog-correction.duration")
                }
            }

            DialogSection {
                LabeledContent("Start") {
                    DatePicker(
                        "Start",
                        selection: Binding(
                            get: { correction.start }, set: { correction.setStart($0) }
                        ), displayedComponents: [.date, .hourAndMinute]
                    )
                    .labelsHidden()
                    .accessibilityIdentifier("worklog-correction.start")
                }
                LabeledContent("End") {
                    if correction.end != nil {
                        DatePicker(
                            "End",
                            selection: Binding(
                                get: { correction.end ?? correction.start },
                                set: { correction.setEnd($0) }
                            ), displayedComponents: [.date, .hourAndMinute]
                        )
                        .labelsHidden()
                        .accessibilityIdentifier("worklog-correction.end")
                    } else {
                        Label("Still running", systemImage: "timer")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .disabled(!correction.canEdit)
        } status: {
            DelayedProgressIndicator(
                isBusy: correction.isSubmitting, title: "Saving worklog...",
                identifier: "worklog-correction.progress")
        } actions: {
            Button("Cancel") { correction.cancel() }
                .accessibilityIdentifier("worklog-correction.cancel")
                .keyboardShortcut(.cancelAction)
                .disabled(correction.isSubmitting)
            Button("Save") { correction.submit() }
                .accessibilityIdentifier("worklog-correction.save")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!correction.canSubmit)
        }
        .environment(\.timeZone, correction.timezone)
    }
}
