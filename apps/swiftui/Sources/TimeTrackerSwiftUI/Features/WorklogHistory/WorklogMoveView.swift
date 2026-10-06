import SwiftUI
import TrackerClient

@MainActor
struct WorklogMoveDialog: View {
    @ObservedObject var move: WorklogMoveStore
    let isPresentationOwner: Bool

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(isPresented: Binding(
                get: { isPresentationOwner && move.state.isPresented },
                set: { if isPresentationOwner && !$0 { move.cancel() } }
            )) {
                WorklogMoveSheet(move: move)
            }
    }
}

@MainActor
private struct WorklogMoveSheet: View {
    @ObservedObject var move: WorklogMoveStore
    @FocusState private var searchFocused: Bool

    var body: some View {
        let state = move.sheetContent
        VStack(alignment: .leading, spacing: 16) {
            Text("Move worklog").font(.title2.bold())
            LabeledContent("From", value: state.sourceTaskName)
                .lineLimit(2)
                .help(state.sourceTaskName)
            if let original = state.original {
                Text("\(localDay(original.start)) · \(localTimestamp(original.start)) to \(original.end.map(localTimestamp) ?? "Running")")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                if original.end == nil {
                    Label("The timer will keep running on the destination task.", systemImage: "timer")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            TextField("Search destination tasks", text: Binding(
                get: { move.sheetContent.query }, set: { move.setQuery($0) }
            ))
                .textFieldStyle(.roundedBorder)
                .disableAutocorrection(true)
                .focused($searchFocused)
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

            Group {
                if state.isSearching {
                    VStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Loading destinations...").foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if state.candidates.isEmpty {
                    Text(state.query.isEmpty ? "No available destination tasks." : "No matching tasks.")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollViewReader { proxy in
                        List(selection: Binding<String?>(
                            get: { move.sheetContent.selectedTaskID },
                            set: { if let taskID = $0 { move.select(taskID: taskID) } }
                        )) {
                            ForEach(state.candidates) { candidate in
                                Text(candidate.name)
                                    .lineLimit(2)
                                    .help(candidate.name)
                                    .tag(candidate.id)
                                    .id(candidate.id)
                            }
                        }
                        .listStyle(.inset)
                        .disabled(!state.canEdit)
                        .onChange(of: state.selectedTaskID) { _, taskID in
                            if let taskID { proxy.scrollTo(taskID) }
                        }
                    }
                }
            }
            .frame(height: 220)

            if let error = state.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                if state.canEdit, state.candidates.isEmpty {
                    Button("Reload destinations") { move.retryCandidates() }
                }
            }

            if state.requiresReview, let latest = state.latest {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Latest saved times").font(.subheadline.weight(.semibold))
                    Text("Start: \(localTimestamp(latest.start))")
                    Text(latest.end.map { "End: \(localTimestamp($0))" } ?? "Still running")
                    Button("Review latest entry") { move.reviewLatest() }
                }
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            }

            HStack(spacing: 12) {
                if state.isSubmitting {
                    ProgressView().controlSize(.small)
                    Text("Moving worklog...").foregroundStyle(.secondary)
                }
                Spacer()
                Button("Cancel") { move.cancel() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(state.isSubmitting)
                Button(state.error != nil && !state.canEdit && state.canSubmit ? "Retry" : "Move") {
                    move.submit()
                }
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

@MainActor
struct PendingWorklogMoveButton: View {
    @ObservedObject var move: WorklogMoveStore

    var body: some View {
        if !move.state.isPresented, let original = move.state.original {
            Button("Retry worklog move") { move.open(worklogID: original.id) }
                .disabled(!move.canOpen)
        }
    }
}
