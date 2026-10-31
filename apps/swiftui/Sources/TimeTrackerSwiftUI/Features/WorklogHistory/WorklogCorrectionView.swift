import SwiftUI
import TrackerClient

struct WorklogCorrectionSheet: View {
    let correction: WorklogCorrectionStore

    private var timezone: TimeZone {
        TimeZone(identifier: correction.sheetContent.timezoneIdentifier) ?? .gmt
    }

    var body: some View {
        let state = correction.sheetContent
        VStack(alignment: .leading, spacing: 16) {
            Text("Edit worklog").font(.title2.bold())
            Text(state.taskName)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(3)
                .help(state.taskName)

            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 16) {
                GridRow {
                    Text("Start")
                    DatePicker(
                        "Start",
                        selection: Binding(
                            get: { correction.sheetContent.start }, set: { correction.setStart($0) }
                        ), displayedComponents: [.date, .hourAndMinute]
                    )
                    .labelsHidden()
                    .accessibilityIdentifier("worklog-correction.start")
                }

                if state.end != nil {
                    GridRow {
                        Text("End")
                        DatePicker(
                            "End",
                            selection: Binding(
                                get: { correction.sheetContent.end ?? correction.sheetContent.start },
                                set: { correction.setEnd($0) }
                            ), displayedComponents: [.date, .hourAndMinute]
                        )
                        .labelsHidden()
                        .accessibilityIdentifier("worklog-correction.end")
                    }
                }
            }
            .disabled(!state.canEdit)

            if state.end == nil {
                Label("This worklog is still running.", systemImage: "timer")
                    .foregroundStyle(.secondary)
            }

            Text("Time zone: \(state.timezoneIdentifier)")
                .accessibilityIdentifier("worklog-correction.timezone")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let end = state.end, end > state.start {
                LabeledContent("Duration") {
                    Text(formatClockDuration(end.timeIntervalSince(state.start)))
                        .accessibilityIdentifier("worklog-correction.duration")
                        .monospacedDigit()
                }
            } else if state.end == nil {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    LabeledContent(
                        "Elapsed after save",
                        value: formatClockDuration(
                            max(0, context.date.timeIntervalSince(state.start))
                        )
                    )
                    .monospacedDigit()
                }
            }

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("worklog-correction.error")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            HStack(spacing: 12) {
                DelayedProgressIndicator(
                    isBusy: state.isSubmitting, title: "Saving worklog...",
                    identifier: "worklog-correction.progress")
                Spacer()
                Button("Cancel") { correction.cancel() }
                    .accessibilityIdentifier("worklog-correction.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button("Save") {
                    correction.submit()
                }
                .accessibilityIdentifier("worklog-correction.save")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!state.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 480)
        .environment(\.timeZone, timezone)
        .interactiveDismissDisabled(state.isSubmitting)
    }
}
