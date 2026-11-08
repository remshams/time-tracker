import SwiftUI
import TrackerClient

struct WorklogCorrectionSheet: View {
    @Environment(WorklogCorrectionStore.self) private var correction

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Edit worklog").font(.title2.bold())
            Text(correction.taskName)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(3)
                .help(correction.taskName)

            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 16) {
                GridRow {
                    Text("Start")
                    DatePicker(
                        "Start",
                        selection: Binding(
                            get: { correction.start }, set: { correction.setStart($0) }
                        ), displayedComponents: [.date, .hourAndMinute]
                    )
                    .labelsHidden()
                    .accessibilityIdentifier("worklog-correction.start")
                }

                if correction.end != nil {
                    GridRow {
                        Text("End")
                        DatePicker(
                            "End",
                            selection: Binding(
                                get: { correction.end ?? correction.start },
                                set: { correction.setEnd($0) }
                            ), displayedComponents: [.date, .hourAndMinute]
                        )
                        .labelsHidden()
                        .accessibilityIdentifier("worklog-correction.end")
                    }
                }
            }
            .disabled(!correction.canEdit)

            if correction.end == nil {
                Label("This worklog is still running.", systemImage: "timer")
                    .foregroundStyle(.secondary)
            }

            Text("Time zone: \(correction.timezone.identifier)")
                .accessibilityIdentifier("worklog-correction.timezone")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let end = correction.end, end > correction.start {
                LabeledContent("Duration") {
                    Text(formatClockDuration(end.timeIntervalSince(correction.start)))
                        .accessibilityIdentifier("worklog-correction.duration")
                        .monospacedDigit()
                }
            } else if correction.end == nil {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    LabeledContent(
                        "Elapsed after save",
                        value: formatClockDuration(
                            max(0, context.date.timeIntervalSince(correction.start))
                        )
                    )
                    .monospacedDigit()
                }
            }

            if let error = correction.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("worklog-correction.error")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: 12) {
                DelayedProgressIndicator(
                    isBusy: correction.isSubmitting, title: "Saving worklog...",
                    identifier: "worklog-correction.progress")
                Spacer()
                Button("Cancel") { correction.cancel() }
                    .accessibilityIdentifier("worklog-correction.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(correction.isSubmitting)
                Button("Save") {
                    correction.submit()
                }
                .accessibilityIdentifier("worklog-correction.save")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!correction.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 480)
        .environment(\.timeZone, correction.timezone)
        .interactiveDismissDisabled(correction.isSubmitting)
    }
}
