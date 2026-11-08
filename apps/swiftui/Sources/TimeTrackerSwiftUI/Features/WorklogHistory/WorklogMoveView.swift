import SwiftUI
import TrackerClient

struct WorklogMoveSheet: View {
    @Environment(WorklogMoveStore.self) private var move
    @FocusState private var searchFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Move worklog").font(.title2.bold())
            LabeledContent("From", value: move.sourceTaskName)
                .lineLimit(2)
                .help(move.sourceTaskName)
            if let original = move.original {
                Text(
                    "\(localDay(original.start)) · \(localTimestamp(original.start)) to \(original.end.map(localTimestamp) ?? "Running")"
                )
                .font(.subheadline)
                .foregroundStyle(.secondary)
                if original.end == nil {
                    Label("The timer will keep running on the destination task.", systemImage: "timer")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            TextField(
                "Search destination tasks",
                text: Binding(
                    get: { move.query }, set: { move.setQuery($0) }
                )
            )
            .textFieldStyle(.roundedBorder)
            .disableAutocorrection(true)
            .focused($searchFocused)
            .accessibilityIdentifier("worklog-move.search")
            .disabled(!move.canEdit)
            .onKeyPress(.upArrow) {
                move.moveSelection(by: -1)
                return .handled
            }
            .onKeyPress(.downArrow) {
                move.moveSelection(by: 1)
                return .handled
            }
            .onSubmit { move.submit() }

            VStack(spacing: 8) {
                DelayedProgressIndicator(
                    isBusy: move.isSearching, title: "Loading destinations...",
                    identifier: "worklog-move.search-progress")
                if move.candidates.isEmpty {
                    if !move.isSearching {
                        Text(move.query.isEmpty ? "No available destination tasks." : "No matching tasks.")
                            .accessibilityIdentifier("worklog-move.empty")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    } else {
                        Spacer()
                    }
                } else {
                    ScrollViewReader { proxy in
                        List(
                            selection: Binding<String?>(
                                get: { move.selectedTaskID },
                                set: { if let taskID = $0 { move.select(taskID: taskID) } }
                            )
                        ) {
                            ForEach(move.candidates) { candidate in
                                Text(candidate.name)
                                    .lineLimit(2)
                                    .help(candidate.name)
                                    .accessibilityIdentifier("worklog-move.candidate.\(candidate.id)")
                                    .tag(candidate.id)
                                    .id(candidate.id)
                            }
                        }
                        .listStyle(.inset)
                        .disabled(!move.canEdit || move.isSearching)
                        .onChange(of: move.selectedTaskID) { _, taskID in
                            if let taskID { proxy.scrollTo(taskID) }
                        }
                    }
                }
            }
            .frame(height: 220)

            if let error = move.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("worklog-move.error")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                if move.canEdit, move.candidates.isEmpty {
                    Button("Reload destinations") { move.retryCandidates() }
                }
            }

            HStack(spacing: 12) {
                DelayedProgressIndicator(
                    isBusy: move.isSubmitting, title: "Moving worklog...",
                    identifier: "worklog-move.submit-progress")
                Spacer()
                Button("Cancel") { move.cancel() }
                    .accessibilityIdentifier("worklog-move.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(move.isSubmitting)
                Button("Move") {
                    move.submit()
                }
                .accessibilityIdentifier("worklog-move.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!move.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 560)
        .interactiveDismissDisabled(move.isSubmitting)
        .onAppear { searchFocused = true }
    }
}
