import SwiftUI
import TrackerClient

struct WorklogMoveSheet: View {
    @Environment(WorklogMoveStore.self) private var move
    @FocusState private var searchFocused: Bool

    var body: some View {
        DialogSheet(
            title: "Move worklog", message: move.sourceTaskName, systemImage: "arrow.right.arrow.left",
            tint: .task(id: move.original?.taskId), width: DialogSize.wideWidth, error: move.error,
            errorIdentifier: "worklog-move.error", isBusy: move.isSubmitting
        ) {
            if let original = move.original, let start = parseTimestamp(original.start) {
                DialogSection {
                    WorklogSpanPanel(start: start, end: parseTimestamp(original.end), taskID: original.taskId)
                } footer: {
                    if original.end == nil {
                        Label("The timer will keep running on the destination task.", systemImage: "timer")
                    }
                }
            }

            DialogSection(field: $searchFocused) {
                HStack(spacing: Spacing.small) {
                    TextField(
                        "Search destination tasks",
                        text: Binding(get: { move.query }, set: { move.setQuery($0) }),
                        prompt: Text("Search destination tasks")
                    )
                    .textFieldStyle(.plain)
                    .font(TextRole.body)
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
                    // The spinner sits in the field's row, so a search doesn't push the list down.
                    DelayedProgressIndicator(
                        isBusy: move.isSearching, identifier: "worklog-move.search-progress")
                }
            }

            MoveDestinations()
        } status: {
            DelayedProgressIndicator(
                isBusy: move.isSubmitting, title: "Moving worklog...",
                identifier: "worklog-move.submit-progress")
        } actions: {
            Button("Cancel") { move.cancel() }
                .accessibilityIdentifier("worklog-move.cancel")
                .keyboardShortcut(.cancelAction)
                .disabled(move.isSubmitting)
            Button("Move") { move.submit() }
                .accessibilityIdentifier("worklog-move.confirm")
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(!move.canSubmit)
        }
        .onAppear { searchFocused = true }
    }
}

/// The tasks the worklog can move to, in a group of fixed height, so the sheet keeps its size while a search runs or
/// finds nothing.
private struct MoveDestinations: View {
    @Environment(WorklogMoveStore.self) private var move

    var body: some View {
        Group {
            if move.candidates.isEmpty {
                if !move.isSearching {
                    VStack(spacing: Spacing.small) {
                        Text(move.query.isEmpty ? "No available destination tasks." : "No matching tasks.")
                            .accessibilityIdentifier("worklog-move.empty")
                            .foregroundStyle(.secondary)
                        if move.error != nil, move.canEdit {
                            Button("Reload destinations") { move.retryCandidates() }
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Color.clear
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
                            HStack(spacing: Spacing.small) {
                                MoveDestinationIndicator(taskID: candidate.id)
                                Text(candidate.name)
                                    .lineLimit(2)
                                    .help(candidate.name)
                                    .accessibilityIdentifier("worklog-move.candidate.\(candidate.id)")
                            }
                            .tag(candidate.id)
                            .id(candidate.id)
                        }
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                    .disabled(!move.canEdit || move.isSearching)
                    .onChange(of: move.selectedTaskID) { _, taskID in
                        if let taskID { proxy.scrollTo(taskID) }
                    }
                }
            }
        }
        .frame(height: DialogSize.listHeight)
        .dialogGroup()
    }
}

/// A destination's color mark, as the sidebar shows it: whether it runs and whether it has time today.
private struct MoveDestinationIndicator: View {
    let taskID: String
    @Environment(TrackingStore.self) private var tracking
    @Environment(DailyTotalsStore.self) private var dailyTotals

    var body: some View {
        TaskIndicatorView(
            taskID: taskID, isRunning: tracking.active?.taskId == taskID,
            hasTimeToday: dailyTotals.taskTotal(taskID: taskID).content?.hasTime == true)
    }
}
