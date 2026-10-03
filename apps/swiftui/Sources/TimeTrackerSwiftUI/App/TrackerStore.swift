import Combine
import Foundation
import TrackerClient

@MainActor
final class TrackerStore: ObservableObject {
    let objectWillChange = ObservableObjectPublisher()
    private let session: TrackerSession
    private var lifecycle: MacLifecycleObserver?

    init() {
        session = TrackerSession(client: TrackerWorker(), clock: SystemTrackerClock(),
                                 scheduler: RunLoopTrackerScheduler(),
                                 settings: UserDefaultsConnectionSettings())
        session.onChange = { [weak self] in self?.objectWillChange.send() }
        lifecycle = MacLifecycleObserver(session: session)
        session.start()
        lifecycle?.start()
    }

    var tasks: [TaskItem] { session.tasks }
    var active: WorklogItem? { session.active }
    var worklogs: [WorklogItem] { session.worklogs }
    var nextCursor: String? { session.nextCursor }
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
    var canStartSelectedTask: Bool { session.canStartSelectedTask }
    var canStopTracking: Bool { session.canStopTracking }
    var runningTaskName: String { session.runningTaskName }
    var timerDisplayText: String { session.timerDisplayText }
    var elapsed: TimeInterval? { session.elapsed }

    func changeTab(_ tab: TaskTab) { session.changeTab(tab) }
    func select(_ taskID: String?) { session.select(taskID) }
    func refresh() { session.refresh() }
    func retryHistory() { session.retryHistory() }
    func loadOlder() { session.loadOlder() }
    func startTracking(taskID: String) { session.startTracking(taskID: taskID) }
    func stopTracking(worklogID: String) { session.stopTracking(worklogID: worklogID) }
    func dismissTrackingError() { session.dismissTrackingError() }
    func testConnection(_ settings: ConnectionSettings) async throws {
        try await session.testConnection(settings)
    }
    func connect(_ settings: ConnectionSettings) async -> Bool {
        await session.connect(settings)
    }
}
