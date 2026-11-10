import Foundation

/// Keeps the stores in step with the tracker: the first load, refreshes, polling, today's report, the display,
/// rollover and menu bar timers, window and menu visibility, sleep and wake, the screen-lock auto-pause, the
/// connection change, and the reloads after dialog writes.
///
/// `TrackerFeatures` creates the stores and passes them in. The stores' closures call the package-internal methods
/// below, for example to switch the connection, reload the task list or the history, record a
/// failure and publish.
///
/// Views don't read it. The app delegate starts it from `TrackerFeatures`, and the menu bar and lifecycle classes get
/// it through `init`; all of them only call its methods.
public final class TrackerSync {
    private let client: any TrackerClient
    private let reports: (any ReportClient)?
    private let clock: any TrackerClock
    private let scheduler: any TrackerScheduler
    private let catalog: TaskCatalogStore
    private let history: WorklogHistoryStore
    private let connection: ConnectionStore
    private let tracking: TrackingStore
    private let dailyTotals: DailyTotalsStore
    private let menuBar: MenuBarStore
    private let automation: TrackingAutomationStore
    private let settingsRepository: any ConnectionSettingsRepository
    private var displayTimer: (any TrackerCancellation)?
    private var pollTimer: (any TrackerCancellation)?
    private var rolloverTimer: (any TrackerCancellation)?
    private var menuBarClock: (any TrackerCancellation)?
    private var menuBarClockEnabled = false
    // The scheduler runs no callback after cancel(), so timer callbacks need no staleness check.
    private var started = false
    // One poll at a time: no poll is scheduled while a refresh is waiting.
    private var refreshing = false
    private var sleeping = false
    private var windowVisible = false
    private var menuOpen = false
    private var uiVisible: Bool { windowVisible || menuOpen }

    init(
        client: any TrackerClient, reports: (any ReportClient)?, clock: any TrackerClock,
        scheduler: any TrackerScheduler, catalog: TaskCatalogStore, history: WorklogHistoryStore,
        connection: ConnectionStore, tracking: TrackingStore, dailyTotals: DailyTotalsStore,
        menuBar: MenuBarStore, automation: TrackingAutomationStore, settings: any ConnectionSettingsRepository
    ) {
        self.client = client
        self.reports = reports
        self.clock = clock
        self.scheduler = scheduler
        self.catalog = catalog
        self.history = history
        self.connection = connection
        self.tracking = tracking
        self.dailyTotals = dailyTotals
        self.menuBar = menuBar
        self.automation = automation
        settingsRepository = settings
    }

    isolated deinit {
        displayTimer?.cancel()
        pollTimer?.cancel()
        rolloverTimer?.cancel()
        menuBarClock?.cancel()
    }

    public func start() {
        guard !started else { return }
        started = true
        dailyTotals.updateDay(at: clock.now)
        updateRolloverTimer()
        updateDisplayTimer()
        refreshing = true
        let settings = connection.settings
        Task { [weak self] in
            guard let self else { return }
            do {
                acceptTaskList(try await client.openConfigured(settings))
                await loadReport()
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

    /// Republishes once a minute while a timer runs, so the menu bar's daily total keeps counting while no
    /// window or menu is open. The app turns this on while the menu bar shows the daily total.
    public func setMenuBarClockEnabled(_ enabled: Bool) {
        menuBarClockEnabled = enabled
        updateMenuBarClock()
    }

    public func sleep() {
        guard started else { return }
        sleeping = true
        cancelTimers()
    }

    public func wake() {
        guard started else { return }
        sleeping = false
        tracking.resetElapsedAnchor(clock: clock)
        dailyTotals.updateDay(at: clock.now)
        dailyTotals.reanchor(clock: clock)
        updateDisplayTimer()
        updateRolloverTimer()
        refresh()
    }

    /// Reloads the task list now unless the sync hasn't started, sleeps or already refreshes. Polling, waking, the
    /// timers and `ConnectionStore.refresh()` call it.
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
        await loadReport()
        schedulePolling()
    }

    /// Pauses the running timer when the auto-pause was just turned on while the screen is locked, and publishes.
    func pauseOnScreenLockChanged() {
        if automation.enabled && automation.locked { pauseTracking(at: clock.now) }
        publish()
    }

    /// Pauses the running timer when the screen locks and resumes its task when the screen unlocks, if the
    /// auto-pause is on. `date` is when the event happened, which the requests send as their time.
    public func setScreenLocked(_ locked: Bool, at date: Date) {
        if locked {
            guard automation.lock() else { return }
            if automation.enabled { pauseTracking(at: date) }
        } else if let resume = automation.unlock(at: date) {
            Task { [weak self] in
                guard let self else { return }
                await resumeTracking(resume)
                await reloadTaskList()
            }
        }
        publish()
    }

    /// Reloads the task list and, with a report client, today's totals. Publishes on every path, which also shows a
    /// failure recorded before this call.
    func reloadTaskList() async {
        dailyTotals.updateDay(at: clock.now)
        if reports == nil || !connection.confirmed || connection.stale {
            do {
                acceptTaskList(try await client.refreshTaskList(settings: connection.settings))
            } catch {
                recordConnectionFailure(error)
                return
            }
        }
        await loadReport()
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

    /// Changes to the tab and selection of a task the tracker just created, and reloads the history once if either
    /// changed. The reload before this call may have failed. The catalog then doesn't hold the task, so `created`
    /// decides the tab, and that tab restores its remembered selection or its first task.
    func showTask(_ created: TaskItem) {
        let confirmed = catalog.tasks.first { $0.id == created.id } ?? created
        let changedTab = catalog.switchTab(to: confirmed.archived ? .archived : .active)
        let changedSelection = catalog.switchSelection(to: created.id)
        if changedTab || changedSelection { history.reload() }
    }

    private func acceptTaskList(_ resources: TaskListResources, requestReport: Bool = true) {
        dailyTotals.updateDay(at: clock.now)
        let retryFailedHistory = connection.stale && history.unavailable
        connection.acceptTaskList()
        menuBar.observe(resources, settings: connection.settings)
        let previousActive = tracking.active
        automation.observeActive(resources.tracking.value)
        tracking.apply(resources.tracking.value, clock: clock)
        if reports != nil && requestReport { dailyTotals.invalidate() }
        let selectionChanged = catalog.apply(
            resources.catalog.value, previousActive: previousActive, active: tracking.active)
        if selectionChanged || retryFailedHistory {
            history.reload()
        } else if !history.unavailable {
            history.clearError()
        }
        updateDisplayTimer()
        updateRolloverTimer()
        publish()
    }

    private func loadReport() async {
        guard let reports, let requested = dailyTotals.begin(clock: clock) else { return }
        // A first request shows the loading status, which the menu reads from the store.
        publish()
        do {
            let refreshed = try await reports.refreshTaskListWithTotals(
                settings: connection.settings, start: requested.start,
                end: requested.end, now: requested.now)
            try dailyTotals.validate(refreshed)
            dailyTotals.accept(refreshed, requested: requested, clock: clock)
            acceptTaskList(refreshed.taskList, requestReport: false)
        } catch {
            dailyTotals.fail(error, clock: clock)
            recordReadFailure(error)
            publish()
        }
    }

    private func recordConnectionFailure(_ failure: Error) {
        connection.recordFailure(failure)
        schedulePolling()
        history.showError(failure.localizedDescription)
        publish()
    }

    /// Recovers the connection when a read failure calls for it.
    func recordReadFailure(_ error: Error) {
        if requiresConnectionRecovery(error) { recordConnectionFailure(error) }
    }

    /// Callers pass read failures, which the bridge never marks as uncertain.
    private func requiresConnectionRecovery(_ error: Error) -> Bool {
        guard let failure = error as? BridgeFailure else { return false }
        return failure.requiresRefresh || failure.kind == "unavailable" || failure.kind == "protocol"
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

    func publish() {
        tracking.publish(clock: clock, confirmed: connection.confirmed)
        automation.publish()
        dailyTotals.publish(tasks: catalog.tasks, active: tracking.active, clock: clock)
    }

    private func finishConnection(_ resources: TaskListResources, settings: ConnectionSettings) {
        automation.cancel()
        connection.apply(settings)
        dailyTotals.clear(at: clock.now)
        settingsRepository.save(settings)
        catalog.resetSelections()
        history.request(selectedTaskID: nil)
        tracking.clearError()
        acceptTaskList(resources)
    }

    private func pauseTracking(at date: Date) {
        guard let worklog = tracking.active, automation.beginPause() else { return }
        tracking.clearError()
        let occurredAt = formatTimestamp(date)
        Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await client.pauseTracking(worklogID: worklog.id, occurredAt: occurredAt)
                if let resume = automation.paused(taskID: worklog.taskId, didStop: result.didStop) {
                    await resumeTracking(resume)
                }
            } catch {
                automation.pauseFailed()
                tracking.recordFailure(error)
            }
            await reloadTaskList()
        }
    }

    /// Only follows a successful pause, which already cleared the tracking error.
    private func resumeTracking(_ resume: TrackingAutomationStore.Resume) async {
        do {
            _ = try await client.resumeTracking(taskID: resume.taskID, occurredAt: formatTimestamp(resume.date))
        } catch {
            tracking.recordFailure(error)
        }
    }

    private func finishRefresh() {
        refreshing = false
        schedulePolling()
    }

    private func updateDisplayTimer() {
        updateMenuBarClock()
        let needed = started && !sleeping && uiVisible && tracking.active != nil
        if !needed {
            cancelDisplay()
            return
        }
        guard displayTimer == nil else { return }
        displayTimer = scheduler.schedule(after: 1, repeating: true, tolerance: 0.2) { [weak self] in
            guard let self else { return }
            if reports != nil, dailyTotals.updateDay(at: clock.now) {
                updateRolloverTimer()
                if !connection.protocolBlocked { refresh() }
            }
            publish()
        }
    }

    private func updateMenuBarClock() {
        guard started, !sleeping, menuBarClockEnabled, tracking.active != nil else {
            menuBarClock?.cancel()
            menuBarClock = nil
            return
        }
        guard menuBarClock == nil else { return }
        menuBarClock = scheduler.schedule(after: 60, repeating: true, tolerance: 5) { [weak self] in
            self?.publish()
        }
    }

    private func cancelTimers() {
        cancelPolling()
        cancelDisplay()
        menuBarClock?.cancel()
        menuBarClock = nil
        rolloverTimer?.cancel()
        rolloverTimer = nil
    }

    private func updateRolloverTimer() {
        rolloverTimer?.cancel()
        rolloverTimer = nil
        guard reports != nil, started, !sleeping, let day = dailyTotals.day else { return }
        rolloverTimer = scheduler.schedule(
            after: max(0.001, day.end.timeIntervalSince(clock.now)),
            repeating: false, tolerance: 0
        ) { [weak self] in
            guard let self else { return }
            dailyTotals.updateDay(at: clock.now)
            updateRolloverTimer()
            if !connection.protocolBlocked { refresh() }
            publish()
        }
    }

    private func cancelPolling() {
        pollTimer?.cancel()
        pollTimer = nil
    }

    private func cancelDisplay() {
        displayTimer?.cancel()
        displayTimer = nil
    }

    private func visibilityChanged(previouslyVisible: Bool) {
        updateDisplayTimer()
        if uiVisible && !previouslyVisible {
            refresh()
        } else if uiVisible != previouslyVisible {
            schedulePolling()
        }
        publish()
    }
}
