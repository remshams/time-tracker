import Foundation
import Observation

/// Holds the Quick Track dialog: whether it shows, the search text, the search result and the highlighted row. It
/// starts and stops tracking from the dialog.
///
/// The dialog reads `content`, which reads the stores through `QuickTrackSources`, so its durations keep ticking. Each
/// search runs through the client, which ranks tasks with the application's search rule. A result for an older
/// query is dropped.
@Observable
public final class QuickTrackStore: ChangeGuardedStore {
    public private(set) var isPresented = false
    public private(set) var query = ""
    private(set) var results: [String] = []
    private(set) var highlight: QuickTrackHighlight?
    private(set) var searchError: String?
    private(set) var createError: String?
    /// Whether a create request from the "Create and start" row is running.
    public private(set) var isCreating = false
    /// Whether a search for the current query is still running.
    public private(set) var isSearching = false
    @ObservationIgnored private var revision = 0
    /// Counts the openings, so a create request that ends after the dialog closed doesn't touch a later opening.
    @ObservationIgnored private var session = 0
    /// Return pressed while a search ran. It runs once the result arrives, so it acts on the current query's match.
    @ObservationIgnored private var activatesAfterSearch = false
    /// Reports to `TrackerSync` whether the dialog shows. `TrackerFeatures` sets it.
    @ObservationIgnored var setVisible: @MainActor (Bool) -> Void = { _ in }
    /// Called when Return can't act on the highlighted row, so the app can beep.
    @ObservationIgnored public var onReject: @MainActor () -> Void = {}
    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let sources: QuickTrackSources
    private let taskCopying: TaskCopying
    private let tracking: TrackingStore
    private let reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void

    init(
        client: any TrackerClient, clock: any TrackerClock, sources: QuickTrackSources, taskCopying: TaskCopying,
        tracking: TrackingStore,
        reloadAfterWrite: @escaping @MainActor (_ then: () -> Void) async -> Void
    ) {
        self.client = client
        self.clock = clock
        self.sources = sources
        self.taskCopying = taskCopying
        self.tracking = tracking
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var content: QuickTrackContent {
        QuickTrackContent(
            sources.read(), query: query, results: results, highlight: highlight,
            messages: [searchError, createError].compactMap { $0 }, isCreating: isCreating)
    }

    /// Shows the dialog with an empty search and nothing highlighted.
    public func open() {
        guard !isPresented else { return }
        set(\.query, "")
        set(\.highlight, nil)
        set(\.searchError, nil)
        set(\.createError, nil)
        activatesAfterSearch = false
        session += 1
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
        set(\.createError, nil)
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
    /// when the highlighted row can't act right now. While a search for a typed query runs, Return waits for its
    /// result, so it acts on the current query's first match.
    public func activate() {
        guard isPresented else { return }
        if isSearching && !QuickTrackContent.isBlank(query) {
            activatesAfterSearch = true
            return
        }
        runHighlightedRow()
    }

    /// Runs a clicked row right away. The row is on screen, so a running search doesn't change what it does.
    public func activate(_ row: QuickTrackHighlight) {
        guard isPresented else { return }
        activatesAfterSearch = false
        // The click comes from an earlier render; a row a newer search or reload removed isn't run.
        guard content.hasRow(row) else { return onReject() }
        set(\.highlight, row)
        runHighlightedRow()
    }

    private func runHighlightedRow() {
        let content = content
        guard !isCreating else { return onReject() }
        if content.highlight == .create {
            guard content.canCreate, let name = content.createName else { return onReject() }
            create(name)
        } else if performHighlightedAction(content) {
            close()
        } else {
            onReject()
        }
    }

    /// Creates a task named `name`, reloads the task list and starts the new task, then closes the dialog. A rejected
    /// name, or a new task that doesn't start, keeps the dialog open with the error and the search text.
    private func create(_ name: String) {
        if let message = TaskNameEditingPolicy.transportError(name) {
            set(\.createError, message)
            return onReject()
        }
        set(\.isCreating, true)
        set(\.createError, nil)
        let occurredAt = formatTimestamp(clock.now)
        Task { [weak self, session] in
            guard let self else { return }
            do {
                let created = try await client.createTask(name: name, occurredAt: occurredAt).task
                await reloadAfterWrite({})
                // The search ran before the task existed; searching again lists it if tracking doesn't start.
                if isPresented && session == self.session { search() }
                guard sources.availability.canStart(taskID: created.id) else {
                    set(\.isCreating, false)
                    return showCreateError("Created \(created.name), but tracking couldn't start.", in: session)
                }
                tracking.startTracking(taskID: created.id) { [weak self] in
                    guard let self else { return }
                    set(\.isCreating, false)
                    if tracking.active?.taskId == created.id {
                        if session == self.session { close() }
                    } else {
                        showCreateError("Created \(created.name), but tracking couldn't start.", in: session)
                    }
                }
            } catch {
                set(\.isCreating, false)
                showCreateError(writeFailure(error).localizedDescription, in: session)
                await reloadAfterWrite({})
                // An uncertain failure may still have created the task; searching again lists it if it exists.
                if isPresented && session == self.session { search() }
            }
        }
    }

    /// Shows a create error only in the opening that sent the request.
    private func showCreateError(_ message: String, in session: Int) {
        guard isPresented, session == self.session else { return }
        set(\.createError, message)
    }

    /// The value a copy action copies for `taskID`, or for the highlighted row when `taskID` is `nil`. The header
    /// copies the running task. Returns `nil` when there's nothing to copy, such as a duration of a task without time
    /// today.
    public func copyValue(_ action: TaskCopyAction, taskID: String? = nil) -> String? {
        guard isPresented, let id = taskID ?? highlightedTaskID else { return nil }
        return taskCopying.value(action, taskID: id)
    }

    private var highlightedTaskID: String? {
        switch content.highlight {
        case .task(let id): return id
        case .header: return tracking.active?.taskId
        case .create, nil: return nil
        }
    }

    private func performHighlightedAction(_ content: QuickTrackContent) -> Bool {
        switch content.highlight {
        case .header:
            guard content.header.canStop, let worklogID = tracking.active?.id else { return false }
            tracking.stopTracking(worklogID: worklogID)
        case .task(let taskID):
            guard content.tasks.first(where: { $0.id == taskID })?.canStart == true else { return false }
            tracking.startTracking(taskID: taskID)
        case .create, nil:
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
        let runsPendingReturn = activatesAfterSearch
        activatesAfterSearch = false
        // A failed search leaves only the create row, so a pending Return must not create a task from it.
        if runsPendingReturn && error == nil { activate() }
    }
}
