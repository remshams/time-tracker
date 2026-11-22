import Foundation
import Observation

/// Holds the Quick Track dialog: whether it shows, the search text, the search result and the highlighted row. It
/// starts and stops tracking from the dialog.
///
/// The dialog reads `content`, which reads the same store values as the menu bar, so its durations keep ticking. Each
/// search runs through the client, which ranks tasks with the application's search rule. A result for an older
/// query is dropped.
@Observable
public final class QuickTrackStore: ChangeGuardedStore {
    public private(set) var isPresented = false
    public private(set) var query = ""
    private(set) var results: [String] = []
    private(set) var highlight: QuickTrackHighlight?
    private(set) var searchError: String?
    /// Whether a search for the current query is still running.
    public private(set) var isSearching = false
    @ObservationIgnored private var revision = 0
    /// Return pressed while a search ran. It runs once the result arrives, so it acts on the current query's match.
    @ObservationIgnored private var activatesAfterSearch = false
    /// Reports to `TrackerSync` whether the dialog shows. `TrackerFeatures` sets it.
    @ObservationIgnored var setVisible: @MainActor (Bool) -> Void = { _ in }
    /// Called when Return can't act on the highlighted row, so the app can beep.
    @ObservationIgnored public var onReject: @MainActor () -> Void = {}
    private let client: any TrackerClient
    private let sources: TrackerMenuSources
    private let tracking: TrackingStore

    init(client: any TrackerClient, sources: TrackerMenuSources, tracking: TrackingStore) {
        self.client = client
        self.sources = sources
        self.tracking = tracking
    }

    public var content: QuickTrackContent {
        QuickTrackContent(
            sources.read(), query: query, results: results, highlight: highlight, searchError: searchError)
    }

    /// Shows the dialog with an empty search and nothing highlighted.
    public func open() {
        guard !isPresented else { return }
        set(\.query, "")
        set(\.highlight, nil)
        set(\.searchError, nil)
        activatesAfterSearch = false
        isPresented = true
        setVisible(true)
        search()
    }

    public func close() {
        guard isPresented else { return }
        isPresented = false
        activatesAfterSearch = false
        // A late result is dropped.
        revision += 1
        set(\.isSearching, false)
        setVisible(false)
    }

    /// Changes the search text. The first match gets the highlight once the result arrives.
    public func setQuery(_ value: String) {
        guard isPresented, value != query else { return }
        query = value
        set(\.highlight, nil)
        activatesAfterSearch = false
        search()
    }

    /// Esc clears the search text, or closes the dialog when the text is empty.
    public func cancel() {
        if query.isEmpty { close() } else { setQuery("") }
    }

    public func moveHighlight(down: Bool) {
        guard isPresented else { return }
        set(\.highlight, content.movingHighlight(down: down))
    }

    /// Highlights a row, for example when the pointer clicks it.
    public func highlight(_ row: QuickTrackHighlight) {
        guard isPresented else { return }
        set(\.highlight, row)
    }

    /// Runs Return: starts the highlighted task or stops the running one, then closes the dialog. Calls `onReject`
    /// when the highlighted row can't act right now.
    public func activate() {
        guard isPresented else { return }
        if isSearching && !QuickTrackContent.isBlank(query) {
            activatesAfterSearch = true
            return
        }
        if performHighlightedAction() { close() } else { onReject() }
    }

    private func performHighlightedAction() -> Bool {
        let content = content
        switch content.highlight {
        case .header:
            guard content.header.canStop, let worklogID = tracking.active?.id else { return false }
            tracking.stopTracking(worklogID: worklogID)
        case .task(let taskID):
            guard content.tasks.first(where: { $0.id == taskID })?.canStart == true else { return false }
            tracking.startTracking(taskID: taskID)
        case nil:
            return false
        }
        return true
    }

    private func search() {
        revision += 1
        set(\.isSearching, true)
        let query = TaskNameEditingPolicy.normalized(query)
        Task { [weak self, revision] in
            guard let self else { return }
            do {
                let found = try await client.searchTasks(query: query)
                finishSearch(revision: revision, results: found, error: nil)
            } catch {
                finishSearch(revision: revision, results: [], error: "Search failed: \(error.localizedDescription)")
            }
        }
    }

    private func finishSearch(revision: Int, results: [String], error: String?) {
        guard isPresented, revision == self.revision else { return }
        set(\.results, results)
        set(\.searchError, error)
        set(\.isSearching, false)
        if activatesAfterSearch {
            activatesAfterSearch = false
            activate()
        }
    }
}
