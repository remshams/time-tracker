import Foundation

/// Keeps the stores in step with the tracker: the first load, refreshes, polling, today's report, the display,
/// rollover and menu bar timers, window and menu visibility, sleep and wake, the screen-lock auto-pause, the
/// connection change, the tab and task selection, and the reloads after dialog writes.
///
/// `TrackerSession` creates the stores and passes them in. Its flows and `TrackingActions` call the
/// package-internal methods below, for example to reload the task list or the history, record a failure and
/// publish.
public final class TrackerSync {
    public var onChange: (() -> Void)?

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
    private let trackingPreferences: any TrackingPreferencesRepository
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
        menuBar: MenuBarStore, automation: TrackingAutomationStore, settings: any ConnectionSettingsRepository,
        trackingPreferences: any TrackingPreferencesRepository
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
        self.trackingPreferences = trackingPreferences
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
        publish()
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

    public func refresh() {
        guard started, !sleeping, !refreshing else { return }
        refreshing = true
        cancelPolling()
        Task { [weak self] in
            guard let self else { return }
            await reloadTaskList()
            finishRefresh()
        }
    }

    public func retryHistory() {
        guard started else { return }
        reloadHistory()
        publish()
    }

    public func loadOlder() {
        guard !connection.stale, history.loadMore(finished: historyPageFinished) else { return }
        publish()
    }

    /// Shows `newTab` and loads the history of the task selected on it.
    public func changeTab(_ newTab: TaskTab) {
        guard catalog.changeTab(newTab) else { return }
        reloadHistory()
        publish()
    }

    /// Selects `taskID` and loads its history.
    public func selectTask(_ taskID: String?) {
        guard catalog.select(taskID) else { return }
        reloadHistory()
        publish()
    }

    /// Switches to `settings`, saves them and loads their task list and today's totals. Returns false when
    /// another change is running or the tracker rejects the settings; the current connection then stays.
    public func connect(_ settings: ConnectionSettings) async -> Bool {
        guard !connection.changing else { return false }
        connection.beginChange()
        publish()
        defer {
            connection.endChange()
            publish()
        }
        do {
            let normalized = try settings.normalized()
            finishConnection(try await client.connect(normalized), settings: normalized)
        } catch {
            connection.rejectChange(error)
            return false
        }
        await loadReport()
        schedulePolling()
        return true
    }

    /// Turns the screen-lock auto-pause on or off and saves the preference. Turning it on while the screen is
    /// locked pauses the running timer.
    public func setPauseOnScreenLock(_ enabled: Bool) {
        guard automation.setEnabled(enabled) else { return }
        trackingPreferences.save(TrackingPreferences(pauseOnScreenLock: enabled))
        if enabled && automation.locked { pauseTracking(at: clock.now) }
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

    /// Drops a pending automatic resume, because a manual tracking request wins over it.
    func cancelAutoResume() { automation.cancel() }

    /// Shows the failure of a tracking request at the next publish.
    func recordTrackingFailure(_ error: Error) {
        tracking.recordError(writeFailure(error).localizedDescription)
    }

    /// Reloads the task list and, with a report client, today's totals.
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

    /// Publishes the write result, reloads the task list, runs `then` and publishes again. A task dialog store
    /// takes this as `reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void`, so pass
    /// `sync.reloadAfterTaskWrite(then:)`.
    func reloadAfterTaskWrite(then: () -> Void = {}) async {
        publish()
        await reloadTaskList()
        then()
        publish()
    }

    /// Publishes the write result, reloads the task list and the history, and publishes again. A worklog dialog
    /// store takes this as `reloadAfterWrite: @MainActor () async -> Void`, so pass `sync.reloadAfterWorklogWrite`.
    func reloadAfterWorklogWrite() async {
        publish()
        await reloadTaskList()
        reloadHistory()
        publish()
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
            reloadHistory()
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
            if requiresConnectionRecovery(error) { recordConnectionFailure(error) }
            publish()
        }
    }

    /// Asks the history store for the selected task's first page.
    func reloadHistory() {
        history.reload(selectedTaskID: catalog.selectedTaskID, finished: historyPageFinished)
    }

    func recordConnectionFailure(_ failure: Error) {
        connection.recordFailure(failure)
        schedulePolling()
        history.showError(failure.localizedDescription)
        publish()
    }

    /// Callers pass read failures, which the bridge never marks as uncertain.
    func requiresConnectionRecovery(_ error: Error) -> Bool {
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
        onChange?()
    }

    /// Publishes each history page or failure and recovers the connection when the failure calls for it.
    private var historyPageFinished: @MainActor (Error?) -> Void {
        { [weak self] failure in
            guard let self else { return }
            if let failure, requiresConnectionRecovery(failure) { recordConnectionFailure(failure) }
            publish()
        }
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
                recordTrackingFailure(error)
            }
            await reloadTaskList()
        }
    }

    /// Only follows a successful pause, which already cleared the tracking error.
    private func resumeTracking(_ resume: TrackingAutomationStore.Resume) async {
        do {
            _ = try await client.resumeTracking(taskID: resume.taskID, occurredAt: formatTimestamp(resume.date))
        } catch {
            recordTrackingFailure(error)
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
