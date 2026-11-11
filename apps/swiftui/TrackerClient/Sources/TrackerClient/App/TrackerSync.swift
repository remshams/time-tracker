import Foundation

/// Keeps the stores in step with the tracker: the first load, refreshes, polling, when to load today's report, window
/// and menu visibility, sleep and wake, the connection change, and the reloads after dialog writes.
///
/// `TrackerFeatures` creates the stores and passes them in. The stores' closures call the package-internal methods
/// below, for example to switch the connection, reload the task list or the history and record a failure. Each store
/// keeps its own observed values current; the sync reports visibility, sleep and wake to the `DisplayClock` that
/// drives the values derived from the clock.
///
/// Views don't read it. The app delegate starts it from `TrackerFeatures`, and the menu bar and lifecycle classes get
/// it through `init`; all of them only call its methods.
public final class TrackerSync {
    private let client: any TrackerClient
    private let scheduler: any TrackerScheduler
    private let catalog: TaskCatalogStore
    private let history: WorklogHistoryStore
    private let connection: ConnectionStore
    private let tracking: TrackingStore
    private let dailyTotals: DailyTotalsStore
    private let menuBar: MenuBarStore
    private let automation: TrackingAutomationStore
    private let settingsRepository: any ConnectionSettingsRepository
    private let displayClock: DisplayClock
    private var pollTimer: (any TrackerCancellation)?
    // The scheduler runs no callback after cancel(), so the poll callback needs no staleness check.
    private var started = false
    // One poll at a time: no poll is scheduled while a refresh is waiting.
    private var refreshing = false
    private var sleeping = false
    private var windowVisible = false
    private var menuOpen = false
    private var uiVisible: Bool { windowVisible || menuOpen }

    init(
        client: any TrackerClient, scheduler: any TrackerScheduler, catalog: TaskCatalogStore,
        history: WorklogHistoryStore, connection: ConnectionStore, tracking: TrackingStore,
        dailyTotals: DailyTotalsStore, menuBar: MenuBarStore, automation: TrackingAutomationStore,
        settings: any ConnectionSettingsRepository, displayClock: DisplayClock
    ) {
        self.client = client
        self.scheduler = scheduler
        self.catalog = catalog
        self.history = history
        self.connection = connection
        self.tracking = tracking
        self.dailyTotals = dailyTotals
        self.menuBar = menuBar
        self.automation = automation
        settingsRepository = settings
        self.displayClock = displayClock
    }

    isolated deinit {
        pollTimer?.cancel()
    }

    public func start() {
        guard !started else { return }
        started = true
        dailyTotals.start()
        displayClock.setAwake(true)
        refreshing = true
        let settings = connection.settings
        Task { [weak self] in
            guard let self else { return }
            do {
                acceptTaskList(try await client.openConfigured(settings))
                await dailyTotals.load()
            } catch {
                recordConnectionFailure(error)
            }
            finishRefresh()
        }
    }

    public func setWindowVisible(_ visible: Bool) {
        let previouslyVisible = uiVisible
        windowVisible = visible
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func setMenuVisible(_ visible: Bool) {
        let previouslyVisible = uiVisible
        menuOpen = visible
        visibilityChanged(previouslyVisible: previouslyVisible)
    }

    public func sleep() {
        guard started else { return }
        sleeping = true
        cancelPolling()
        displayClock.setAwake(false)
        dailyTotals.sleep()
    }

    public func wake() {
        guard started else { return }
        sleeping = false
        tracking.resetElapsedAnchor()
        dailyTotals.wake()
        displayClock.setAwake(true)
        refresh()
    }

    /// Reloads the task list now unless the sync hasn't started, sleeps or already refreshes. Polling, waking,
    /// `ConnectionStore.refresh()` and `DailyTotalsStore` at midnight call it.
    func refresh() {
        guard started, !sleeping, !refreshing else { return }
        refreshing = true
        cancelPolling()
        Task { [weak self] in
            guard let self else { return }
            await reloadTaskList()
            finishRefresh()
        }
    }

    /// Opens `settings`, which are already normalized, resets the stores for them, saves them and loads their task
    /// list and today's totals. Throws when the tracker rejects the settings. `ConnectionStore.connect(_:)` calls it
    /// while it marks the change as running.
    func switchConnection(to settings: ConnectionSettings) async throws {
        finishConnection(try await client.connect(settings), settings: settings)
        await dailyTotals.load()
        schedulePolling()
    }

    /// Reloads the task list and, with a report client, today's totals.
    func reloadTaskList() async {
        if !dailyTotals.loadsTaskList || !connection.confirmed || connection.stale {
            do {
                acceptTaskList(try await client.refreshTaskList(settings: connection.settings))
            } catch {
                recordConnectionFailure(error)
                return
            }
        }
        await dailyTotals.load()
    }

    /// Reloads the task list, then runs `then`, which changes only the tab, the selection or the history.
    func reloadAfterTaskWrite(then: () -> Void = {}) async {
        await reloadTaskList()
        then()
    }

    /// Reloads the task list, then the history.
    func reloadAfterWorklogWrite() async {
        await reloadTaskList()
        history.reload()
    }

    /// Takes in the task list that came with today's report. `DailyTotalsStore` calls it.
    func acceptReportedTaskList(_ resources: TaskListResources) {
        acceptTaskList(resources, requestReport: false)
    }

    private func acceptTaskList(_ resources: TaskListResources, requestReport: Bool = true) {
        let connectionWasStale = connection.stale
        connection.acceptTaskList()
        let previousActive = tracking.active
        automation.acceptTaskList(active: resources.tracking.value)
        tracking.acceptTaskList(active: resources.tracking.value)
        menuBar.acceptTaskList(resources, settings: connection.settings)
        let historyMayHaveChanged = catalog.acceptTaskList(
            resources.catalog.value, previousActive: previousActive, active: tracking.active)
        dailyTotals.acceptTaskList(reportRequested: requestReport)
        history.acceptTaskList(historyMayHaveChanged: historyMayHaveChanged, connectionWasStale: connectionWasStale)
    }

    private func recordConnectionFailure(_ failure: Error) {
        connection.recordFailure(failure)
        schedulePolling()
        history.showError(failure.localizedDescription)
    }

    /// Recovers the connection when a read failure calls for it.
    func recordReadFailure(_ error: Error) {
        if connection.requiresRecovery(after: error) { recordConnectionFailure(error) }
    }

    private func schedulePolling() {
        cancelPolling()
        guard started, !sleeping, !refreshing, !connection.protocolBlocked else { return }
        let interval = TrackerPollingPolicy.interval(visible: uiVisible, failures: connection.failures)
        pollTimer = scheduler.schedule(after: interval, repeating: false, tolerance: min(5, interval * 0.2)) {
            [weak self] in
            self?.refresh()
        }
    }

    private func finishConnection(_ resources: TaskListResources, settings: ConnectionSettings) {
        automation.acceptConnection()
        connection.acceptConnection(settings)
        dailyTotals.acceptConnection()
        settingsRepository.save(settings)
        catalog.acceptConnection()
        history.acceptConnection()
        tracking.acceptConnection()
        acceptTaskList(resources)
    }

    private func finishRefresh() {
        refreshing = false
        schedulePolling()
    }

    private func cancelPolling() {
        pollTimer?.cancel()
        pollTimer = nil
    }

    private func visibilityChanged(previouslyVisible: Bool) {
        displayClock.setVisible(uiVisible)
        if uiVisible && !previouslyVisible {
            refresh()
        } else if uiVisible != previouslyVisible {
            schedulePolling()
        }
    }
}
