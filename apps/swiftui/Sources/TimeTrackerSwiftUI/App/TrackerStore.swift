import Combine
import Foundation
import TrackerClient

@MainActor
final class TrackerStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private let presentation: TrackerPresentationObserver
    private let menuBarPreferences: UserDefaultsMenuBarPreferences
    private(set) var menuBarDisplay: MenuBarDisplay
    private let keyboardPreferences: UserDefaultsMenuKeyboardPreferences
    private(set) var menuShortcuts: MenuKeyboardShortcuts
    private var menuShortcutValidationError: String?
    private(set) var menuGlobalShortcutError: String?
    var registerMenuShortcut: ((MenuShortcut) -> String?)?
    var menuShortcutError: String? { menuShortcutValidationError ?? menuGlobalShortcutError }
    let activity: TrackerActivityStore
    let timer: TrackerTimerStore
    let dailyTotals: TrackerDailyTotalsStore
    let menu: TrackerMenuStore
    let creation: TaskCreationStore
    let rename: TaskRenameStore
    let bulkArchiving: BulkTaskArchivingStore
    let archiving: TaskArchivingStore
    let correction: WorklogCorrectionStore
    let move: WorklogMoveStore
    private var lifecycle: MacLifecycleObserver?

    init() {
        let launch = TrackerLaunchConfiguration.current
        menuBarPreferences = UserDefaultsMenuBarPreferences(defaults: launch.defaults)
        menuBarDisplay = menuBarPreferences.load()
        keyboardPreferences = UserDefaultsMenuKeyboardPreferences(defaults: launch.defaults)
        menuShortcuts = keyboardPreferences.load()
        let worker = TrackerWorker(localDatabasePath: launch.localDatabasePath)
        session = TrackerSession(
            client: worker, clock: SystemTrackerClock(),
            scheduler: RunLoopTrackerScheduler(),
            settings: UserDefaultsConnectionSettings(defaults: launch.defaults),
            trackingPreferences: UserDefaultsTrackingPreferences(defaults: launch.defaults),
            lastTrackedTasks: UserDefaultsLastTrackedTasks(defaults: launch.defaults),
            reports: worker)
        presentation = TrackerPresentationObserver(session: session)
        activity = TrackerActivityStore(session: session, presentation: presentation)
        timer = TrackerTimerStore(session: session, presentation: presentation)
        dailyTotals = TrackerDailyTotalsStore(presentation: presentation)
        menu = TrackerMenuStore(session: session, display: menuBarDisplay)
        creation = TaskCreationStore(session: session, presentation: presentation)
        rename = TaskRenameStore(session: session, presentation: presentation)
        bulkArchiving = BulkTaskArchivingStore(session: session, presentation: presentation)
        archiving = TaskArchivingStore(session: session, presentation: presentation)
        correction = WorklogCorrectionStore(session: session, presentation: presentation)
        move = WorklogMoveStore(session: session, presentation: presentation)
        dailyTotals.onMenuBarTick = { [weak self] in self?.menu.update() }
        presentation.onContentChange = { [weak self] in self?.objectWillChange.send() }
        session.onChange = { [weak self] in
            guard let self else { return }
            updateMenuBarTimer()
            presentation.update(from: session)
            menu.update()
        }
        lifecycle = MacLifecycleObserver(session: session)
        lifecycle?.start()
        session.start()
    }

    var tasks: [TaskItem] { session.tasks }
    var active: WorklogItem? { session.active }
    var worklogs: [WorklogItem] { session.worklogs }
    var error: String? { session.error }
    var trackingError: String? { session.trackingError }
    var historyUnavailable: Bool { session.historyUnavailable }
    var selectedTaskID: String? { session.selectedTaskID }
    var tab: TaskTab { session.tab }
    var connectionSettings: ConnectionSettings { session.connectionSettings }
    var connectionStatusText: String { session.connectionStatusText }
    var connectionMessage: String? { session.connectionMessage }
    var isBusy: Bool { session.isBusy }
    var isChangingConnection: Bool { session.isChangingConnection }
    var isStale: Bool { session.isStale }
    var visibleTasks: [TaskItem] { session.visibleTasks }
    var selectedTask: TaskItem? { session.selectedTask }
    var hasMoreHistory: Bool { session.hasMoreHistory }
    var canStopTracking: Bool { session.canStopTracking }
    var runningTaskName: String { session.runningTaskName }
    var elapsed: TimeInterval? { session.elapsed }
    var pauseOnScreenLock: Bool { session.pauseOnScreenLock }
    var autoPauseStatusText: String? { session.autoPauseStatusText }
    var dailyTotalsStatus: DailyTotalsStatus { session.dailyTotalsStatus }

    func setMenuBarDisplay(_ display: MenuBarDisplay) {
        guard menuBarDisplay != display else { return }
        objectWillChange.send()
        menuBarDisplay = display
        menuBarPreferences.save(display)
        menu.setDisplay(display)
        updateMenuBarTimer()
    }

    private func updateMenuBarTimer() {
        dailyTotals.updateMenuBarTimer(isRunning: menuBarDisplay == .time && session.active != nil)
    }

    func setPauseOnScreenLock(_ enabled: Bool) { session.setPauseOnScreenLock(enabled) }
    func setMenuShortcut(_ action: MenuShortcutAction, shortcut: MenuShortcut) {
        var candidate = action == .openMenu ? MenuKeyboardShortcuts() : menuShortcuts
        candidate[action] = shortcut
        setMenuShortcuts(candidate)
    }

    func resetMenuShortcuts() { setMenuShortcuts(.defaults, retryGlobal: true) }

    private func setMenuShortcuts(_ candidate: MenuKeyboardShortcuts, retryGlobal: Bool = false) {
        if let error = candidate.validationError {
            reportMenuShortcutError(error)
            return
        }
        let changesGlobal = retryGlobal || candidate.openMenu != menuShortcuts.openMenu
        if changesGlobal {
            if let error = registerMenuShortcut?(candidate.openMenu) {
                reportMenuShortcutError(error)
                return
            }
        }
        objectWillChange.send()
        menuShortcuts = candidate
        menuShortcutValidationError = nil
        if changesGlobal { menuGlobalShortcutError = nil }
        keyboardPreferences.save(candidate)
    }

    func reportMenuShortcutError(_ message: String?) {
        guard menuShortcutValidationError != message else { return }
        objectWillChange.send()
        menuShortcutValidationError = message
    }

    func reportMenuGlobalShortcutError(_ message: String?) {
        guard menuGlobalShortcutError != message else { return }
        objectWillChange.send()
        menuGlobalShortcutError = message
    }

    func menuCopyValue(
        _ action: MenuShortcutAction, taskID: String,
        connection: ConnectionSettings
    ) -> String? {
        session.menuCopyValue(action, taskID: taskID, connection: connection)
    }
    func changeTab(_ tab: TaskTab) { session.changeTab(tab) }
    func select(_ taskID: String?) { session.select(taskID) }
    func refresh() { session.refresh() }
    func retryHistory() { session.retryHistory() }
    func loadOlder() { session.loadOlder() }
    func startTracking(taskID: String) { session.startTracking(taskID: taskID) }
    func stopTracking(worklogID: String) { session.stopTracking(worklogID: worklogID) }
    func dismissTrackingError() { session.dismissTrackingError() }
    func performMenuPrimaryAction() -> MenuPrimaryAction { session.performMenuPrimaryAction() }

    func menuOpened() {
        menu.menuOpened()
        session.menuOpened()
    }

    func menuClosed() {
        session.menuClosed()
        menu.menuClosed()
    }
    func testConnection(_ settings: ConnectionSettings) async throws {
        try await session.testConnection(settings)
    }
    func connect(_ settings: ConnectionSettings) async -> Bool {
        await session.connect(settings)
    }
}

@MainActor
final class TrackerDailyTotalsStore {
    var onMenuBarTick: (() -> Void)?
    private let presentation: TrackerPresentationObserver
    private var taskStores: [String: TrackerTaskDailyTotalStore] = [:]
    private var menuBarTimer: Timer?

    init(presentation: TrackerPresentationObserver) {
        self.presentation = presentation
        presentation.onTaskDailyTotalsChange = { [weak self] taskIDs in
            guard let self else { return }
            for id in taskIDs {
                if let content = self.presentation.taskDailyTotals[id] {
                    taskStores[id]?.update(content)
                } else {
                    taskStores.removeValue(forKey: id)
                }
            }
        }
    }

    deinit { menuBarTimer?.invalidate() }

    func updateMenuBarTimer(isRunning: Bool) {
        guard isRunning else {
            menuBarTimer?.invalidate()
            menuBarTimer = nil
            return
        }
        guard menuBarTimer == nil else { return }
        let timer = Timer(timeInterval: 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.onMenuBarTick?() }
        }
        timer.tolerance = 5
        menuBarTimer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    func task(_ taskID: String) -> TrackerTaskDailyTotalStore {
        if let existing = taskStores[taskID] { return existing }
        let store = TrackerTaskDailyTotalStore(content: presentation.taskDailyTotals[taskID])
        taskStores[taskID] = store
        return store
    }
}

@MainActor
final class TrackerTaskDailyTotalStore {
    let objectWillChange = ObservableObjectPublisher()
    private(set) var content: TaskDailyTotalPresentation?

    init(content: TaskDailyTotalPresentation?) { self.content = content }

    func update(_ next: TaskDailyTotalPresentation) {
        guard content != next else { return }
        content = next
        objectWillChange.send()
    }
}

@MainActor
final class TrackerMenuStore {
    let label: TrackerMenuLabelStore
    let valuesDidChange = PassthroughSubject<TrackerMenuValues, Never>()
    private let session: TrackerSession
    private let presentation: TrackerMenuPresentationObserver
    private var display: MenuBarDisplay

    init(session: TrackerSession, display: MenuBarDisplay) {
        self.session = session
        self.display = display
        presentation = TrackerMenuPresentationObserver(session: session, display: display)
        label = TrackerMenuLabelStore(presentation: presentation)
        presentation.onValuesChange = { [weak self] in
            guard let self else { return }
            valuesDidChange.send(presentation.values)
        }
    }

    var content: TrackerMenuContent { presentation.content }
    var values: TrackerMenuValues { presentation.values }

    func update() { presentation.update(from: session, display: display) }

    func setDisplay(_ display: MenuBarDisplay) {
        self.display = display
        update()
    }

    func menuOpened() { presentation.menuOpened(from: session, display: display) }
    func menuClosed() { presentation.menuClosed(from: session, display: display) }
}

@MainActor
final class TrackerMenuLabelStore {
    let objectWillChange = ObservableObjectPublisher()
    private let presentation: TrackerMenuPresentationObserver

    init(presentation: TrackerMenuPresentationObserver) {
        self.presentation = presentation
        presentation.onLabelChange = { [weak self] in self?.objectWillChange.send() }
    }

    var content: TrackerMenuLabelContent { presentation.label }
}

@MainActor
final class TrackerActivityStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onActivityChange = { [weak self] in self?.objectWillChange.send() }
    }

    var isBlockingControls: Bool { session.isBlockingControls }
    var canStartSelectedTask: Bool { session.canStartSelectedTask }
    var canStopTracking: Bool { session.canStopTracking }
    func canStartTracking(taskID: String) -> Bool { session.canStartTracking(taskID: taskID) }
}

@MainActor
final class TrackerTimerStore {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession

    init(session: TrackerSession, presentation: TrackerPresentationObserver) {
        self.session = session
        presentation.onTimerChange = { [weak self] in self?.objectWillChange.send() }
    }

    var text: String { session.timerDisplayText }
}

// Keep publisher access on the main actor with compilers that support isolated conformances.
#if compiler(>=6.2)
    extension TrackerStore: @MainActor ObservableObject {}
    extension TrackerTaskDailyTotalStore: @MainActor ObservableObject {}
    extension TrackerMenuLabelStore: @MainActor ObservableObject {}
    extension TrackerActivityStore: @MainActor ObservableObject {}
    extension TrackerTimerStore: @MainActor ObservableObject {}
#else
    extension TrackerStore: ObservableObject {}
    extension TrackerTaskDailyTotalStore: ObservableObject {}
    extension TrackerMenuLabelStore: ObservableObject {}
    extension TrackerActivityStore: ObservableObject {}
    extension TrackerTimerStore: ObservableObject {}
#endif
