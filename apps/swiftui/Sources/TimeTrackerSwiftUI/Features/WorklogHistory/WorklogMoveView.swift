import SwiftUI
import TrackerClient

@MainActor
struct WorklogMoveDialog: View {
    let move: WorklogMoveStore
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(
                isPresented: Binding(
                    get: { isPresentationOwner && move.state.isPresented },
                    set: { if isPresentationOwner && !$0 { move.cancel() } }
                )
            ) {
                WorklogMoveSheet(move: move)
            }
    }
}

@MainActor
private struct WorklogMoveSheet: View {
    let move: WorklogMoveStore
    @FocusState private var searchFocused: Bool

    var body: some View {
        let state = move.sheetContent
        VStack(alignment: .leading, spacing: 16) {
            Text("Move worklog").font(.title2.bold())
            LabeledContent("From", value: state.sourceTaskName)
                .lineLimit(2)
                .help(state.sourceTaskName)
            if let original = state.original {
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
                    get: { move.sheetContent.query }, set: { move.setQuery($0) }
                )
            )
            .textFieldStyle(.roundedBorder)
            .disableAutocorrection(true)
            .focused($searchFocused)
            .accessibilityIdentifier("worklog-move.search")
            .disabled(!state.canEdit)
            .onKeyPress(.upArrow) {
                move.moveSelection(by: -1)
                return .handled
            }
            .onKeyPress(.downArrow) {
                move.moveSelection(by: 1)
                return .handled
            }
            .onSubmit { if move.state.canSubmit { move.submit() } }

            VStack(spacing: 8) {
                DelayedProgressIndicator(
                    isBusy: state.isSearching, title: "Loading destinations...",
                    identifier: "worklog-move.search-progress")
                if state.candidates.isEmpty {
                    if !state.isSearching {
                        Text(state.query.isEmpty ? "No available destination tasks." : "No matching tasks.")
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
                                get: { move.sheetContent.selectedTaskID },
                                set: { if let taskID = $0 { move.select(taskID: taskID) } }
                            )
                        ) {
                            ForEach(state.candidates) { candidate in
                                Text(candidate.name)
                                    .lineLimit(2)
                                    .help(candidate.name)
                                    .accessibilityIdentifier("worklog-move.candidate.\(candidate.id)")
                                    .tag(candidate.id)
                                    .id(candidate.id)
                            }
                        }
                        .listStyle(.inset)
                        .disabled(!state.canEdit || state.isSearching)
                        .onChange(of: state.selectedTaskID) { _, taskID in
                            if let taskID { proxy.scrollTo(taskID) }
                        }
                    }
                }
            }
            .frame(height: 220)

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .accessibilityIdentifier("worklog-move.error")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                if state.canEdit, state.candidates.isEmpty {
                    Button("Reload destinations") { move.retryCandidates() }
                }
            }

            HStack(spacing: 12) {
                DelayedProgressIndicator(
                    isBusy: state.isSubmitting, title: "Moving worklog...",
                    identifier: "worklog-move.submit-progress")
                Spacer()
                Button("Cancel") { move.cancel() }
                    .accessibilityIdentifier("worklog-move.cancel")
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button("Move") {
                    move.submit()
                }
                .accessibilityIdentifier("worklog-move.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!state.canSubmit)
            }
        }
        .padding(24)
        .frame(width: 560)
        .interactiveDismissDisabled(state.isSubmitting)
        .onAppear { searchFocused = true }
    }
}
