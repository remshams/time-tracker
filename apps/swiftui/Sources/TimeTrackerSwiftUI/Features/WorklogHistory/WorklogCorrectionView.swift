import SwiftUI
import TrackerClient

@MainActor
struct WorklogCorrectionDialog: View {
    @ObservedObject var correction: WorklogCorrectionStore
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(isPresented: Binding(
                get: { isPresentationOwner && correction.state.isPresented },
                set: { if isPresentationOwner && !$0 { correction.cancel() } }
            )) {
                WorklogCorrectionSheet(correction: correction)
            }
    }
}

@MainActor
private struct WorklogCorrectionSheet: View {
    @ObservedObject var correction: WorklogCorrectionStore

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
                    DatePicker("Start", selection: Binding(
                        get: { correction.sheetContent.start }, set: { correction.setStart($0) }
                    ), displayedComponents: [.date, .hourAndMinute])
                        .labelsHidden()
                }

                if state.end != nil {
                    GridRow {
                        Text("End")
                        DatePicker("End", selection: Binding(
                            get: { correction.sheetContent.end ?? correction.sheetContent.start },
                            set: { correction.setEnd($0) }
                        ), displayedComponents: [.date, .hourAndMinute])
                            .labelsHidden()
                    }
                }
            }
            .disabled(!state.canEdit)

            if state.end == nil {
                Label("This worklog is still running.", systemImage: "timer")
                    .foregroundStyle(.secondary)
            }

            Text("Time zone: \(state.timezoneIdentifier)")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let end = state.end, end >= state.start {
                LabeledContent("Duration", value: clockDuration(end.timeIntervalSince(state.start)))
                    .monospacedDigit()
            } else if state.end == nil {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    LabeledContent("Elapsed after save", value: clockDuration(
                        max(0, context.date.timeIntervalSince(state.start))
                    ))
                    .monospacedDigit()
                }
            }

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }

            if state.requiresReview, let latest = state.latest {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Latest saved times").font(.subheadline.weight(.semibold))
                    Text("Start: \(formatted(latest.start))")
                    Text(latest.end.map { "End: \(formatted($0))" } ?? "Still running")
                    Button("Review latest entry") { correction.reviewLatest() }
                        .disabled(state.isSubmitting)
                }
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            }

            HStack(spacing: 12) {
                if state.isSubmitting {
                    ProgressView().controlSize(.small)
                    Text("Saving worklog...").foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel") { correction.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button(state.error != nil && !state.canEdit && state.canSubmit ? "Retry" : "Save") {
                    correction.submit()
                }
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

    private func formatted(_ value: String) -> String {
        guard let date = timestamp(value) else { return value }
        return date.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened, timeZone: timezone))
    }
}

@MainActor
struct WorklogHistoryHeading: View {
    @ObservedObject var correction: WorklogCorrectionStore
    @ObservedObject var move: WorklogMoveStore

    var body: some View {
        HStack {
            Text("Worklogs").font(.title3.weight(.semibold))
            Spacer()
            PendingWorklogCorrectionButton(correction: correction)
            PendingWorklogMoveButton(move: move)
        }
        .padding(.bottom, 4)
    }
}

@MainActor
struct PendingWorklogCorrectionButton: View {
    @ObservedObject var correction: WorklogCorrectionStore

    var body: some View {
        if !correction.state.isPresented, let original = correction.state.original {
            Button("Retry worklog edit") { correction.open(worklogID: original.id) }
                .disabled(!correction.canOpen)
        }
    }
}
