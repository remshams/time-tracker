import Foundation

public final class TrackerSession {
    /// Loads, refreshes and polls the stores, runs the timers, the screen-lock auto-pause and the connection
    /// change, changes the tab and task selection, reloads after dialog writes, and publishes the values derived from
    /// the clock and the recorded errors.
    public let sync: TrackerSync
    /// Starts and stops tracking.
    public let trackingActions: TrackingActions
    /// Views and the menu bar read whether tracking can start or stop here.
    public let trackingAvailability: TrackingAvailability
    /// Views read the task list, tab and selection here; the sync writes them.
    public let taskCatalog = TaskCatalogStore()
    /// Views read the selected task's worklog pages and the history panel's state here; the store loads the pages
    /// when the sync asks it to.
    public let worklogHistory: WorklogHistoryStore
    /// Views read the connection settings and status here; the sync writes them.
    public let connection: ConnectionStore
    /// Views read the running worklog, the tracking error and the timer text here; the sync and `trackingActions`
    /// write them.
    public let tracking = TrackingStore()
    /// Views read today's status and task totals here; the sync writes them.
    public let dailyTotals: DailyTotalsStore
    /// The menu bar reads the last tracked task here; the sync writes it.
    public let menuBar: MenuBarStore
    /// Views read the auto-pause preference and status here; the sync writes them.
    public let trackingAutomation: TrackingAutomationStore
    /// The task rename dialog.
    public let taskRename: TaskRenameStore
    /// The task archive dialog.
    public let taskArchiving: TaskArchivingStore
    /// The task creation dialog.
    public let taskCreation: TaskCreationStore
    /// The worklog deletion dialog.
    public let worklogDeletion: WorklogDeletionStore
    /// The worklog move dialog.
    public let worklogMove: WorklogMoveStore
    /// The bulk archive dialog.
    public let bulkTaskArchiving: BulkTaskArchivingStore
    /// The worklog correction dialog.
    public let worklogCorrection: WorklogCorrectionStore

    // Tests compare the published store values with values computed from this clock.
    let clock: any TrackerClock

    public init(
        client: any TrackerClient, clock: any TrackerClock,
        scheduler: any TrackerScheduler, settings: any ConnectionSettingsRepository,
        trackingPreferences: any TrackingPreferencesRepository,
        lastTrackedTasks: (any LastTrackedTaskRepository)? = nil,
        reports: (any ReportClient)? = nil, calendar: Calendar = .autoupdatingCurrent
    ) {
        worklogHistory = WorklogHistoryStore(client: client)
        dailyTotals = DailyTotalsStore(calendar: calendar)
        self.clock = clock
        trackingAutomation = TrackingAutomationStore(preferences: trackingPreferences.load())
        let initialSettings = settings.load() ?? .local
        connection = ConnectionStore(settings: initialSettings)
        menuBar = MenuBarStore(repository: lastTrackedTasks, settings: initialSettings)
        dailyTotals.updateDay(at: clock.now)
        trackingAvailability = TrackingAvailability(connection: connection, catalog: taskCatalog, tracking: tracking)
        sync = TrackerSync(
            client: client, reports: reports, clock: clock, scheduler: scheduler, catalog: taskCatalog,
            history: worklogHistory, connection: connection, tracking: tracking, dailyTotals: dailyTotals,
            menuBar: menuBar, automation: trackingAutomation, settings: settings,
            trackingPreferences: trackingPreferences)
        trackingActions = TrackingActions(
            client: client, clock: clock, sync: sync, catalog: taskCatalog, availability: trackingAvailability,
            tracking: tracking, menuBar: menuBar, automation: trackingAutomation)
        taskRename = TaskRenameStore(
            client: client, clock: clock, connection: connection, catalog: taskCatalog,
            reloadAfterWrite: sync.reloadAfterTaskWrite(then:))
        taskArchiving = TaskArchivingStore(
            client: client, clock: clock, connection: connection, catalog: taskCatalog, tracking: tracking,
            reloadAfterWrite: sync.reloadAfterTaskWrite(then:))
        taskCreation = TaskCreationStore(
            client: client, clock: clock, connection: connection, showTask: sync.showTask(_:),
            reloadAfterWrite: sync.reloadAfterTaskWrite(then:))
        worklogDeletion = WorklogDeletionStore(
            client: client, connection: connection, catalog: taskCatalog, history: worklogHistory,
            reloadAfterWrite: sync.reloadAfterWorklogWrite)
        worklogMove = WorklogMoveStore(
            client: client, connection: connection, catalog: taskCatalog, history: worklogHistory, tracking: tracking,
            reloadAfterWrite: sync.reloadAfterWorklogWrite)
        bulkTaskArchiving = BulkTaskArchivingStore(
            client: client, clock: clock, connection: connection,
            recordPreviewFailure: sync.recordReadFailure,
            reloadAfterWrite: sync.reloadAfterTaskWrite(then:))
        worklogCorrection = WorklogCorrectionStore(
            client: client, clock: clock, connection: connection, catalog: taskCatalog, history: worklogHistory,
            tracking: tracking, reloadAfterWrite: sync.reloadAfterWorklogWrite)
    }

    // The menu's copy actions and `dailyDuration` read these two.
    public var tasks: [TaskItem] { taskCatalog.tasks }
    public var active: WorklogItem? { tracking.active }

    /// Computed from the clock on each read, so copying from the menu uses the current time.
    func dailyDuration(taskID: String) -> TimeInterval? {
        dailyTotals.duration(taskID: taskID, active: active, clock: clock)
    }
}

public extension TrackerMenuSources {
    /// Reads the menu's stores and tracking checks from the session.
    init(session: TrackerSession) {
        self.init(
            catalog: session.taskCatalog, connection: session.connection, tracking: session.tracking,
            dailyTotals: session.dailyTotals, menuBar: session.menuBar, automation: session.trackingAutomation,
            availability: session.trackingAvailability)
    }
}
