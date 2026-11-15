import Foundation
import Observation

/// Holds the running worklog, the tracking error, the in-flight flag and the timer text for the whole app, and starts
/// and stops tracking from the window and the menu bar.
///
/// Views read the public properties, start and stop tracking and dismiss the error; `TrackerSync` hands in each loaded
/// task list. `TrackingAvailability` decides which requests are possible right now. Each observed property is assigned
/// only when its value changes, so a `DisplayClock` tick redraws the timer text only when the shown second changes.
@Observable
public final class TrackingStore: ChangeGuardedStore {
    public private(set) var active: WorklogItem?
    public private(set) var error: String? {
        didSet { gate.update(self, kind: .alert, isShown: error != nil) }
    }
    public private(set) var isSubmitting = false
    /// The elapsed time of the running worklog, "Idle", or "Unavailable" before the tracker state is confirmed.
    public private(set) var timerText = "Unavailable"
    /// The whole seconds of the running worklog, or `nil` while nothing runs or before the tracker state is confirmed.
    public private(set) var elapsedSeconds: Int?
    // A failed tracking request, auto-pause or auto-resume records its error here. The flow that sent the request
    // shows it once its reload returns, so the alert appears over the reloaded state.
    @ObservationIgnored private(set) var recordedError: String?
    // The elapsed time is measured from these with the monotonic uptime, so views don't observe them.
    @ObservationIgnored private var timerBase: TimeInterval = 0
    @ObservationIgnored private var timerAnchor: TimeInterval = 0
    /// Cancels a pending auto-resume before a tracking request. `TrackerFeatures` sets it to
    /// `TrackingAutomationStore`'s cancel.
    @ObservationIgnored var cancelAutoResume: @MainActor () -> Void = {}
    /// Reloads the task list after a tracking request. `TrackerFeatures` sets it to `TrackerSync`'s reload.
    @ObservationIgnored var reloadTaskList: @MainActor () async -> Void = {}
    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let connection: ConnectionStore
    private let catalog: TaskCatalogStore
    private let gate: PresentationGate
    private let displayClock: DisplayClock

    init(
        client: any TrackerClient, clock: any TrackerClock, connection: ConnectionStore, catalog: TaskCatalogStore,
        gate: PresentationGate, displayClock: DisplayClock
    ) {
        self.client = client
        self.clock = clock
        self.connection = connection
        self.catalog = catalog
        self.gate = gate
        self.displayClock = displayClock
        displayClock.onTick { [weak self] in self?.refreshTimerText() }
    }

    private var availability: TrackingAvailability {
        TrackingAvailability(connection: connection, catalog: catalog, tracking: self)
    }

    /// Starts tracking `taskID`, which stops the running worklog first, and calls `finished` once the request and
    /// its reload end. Does nothing and returns `false` when `TrackingAvailability.canStart(taskID:)` is false.
    @discardableResult
    public func startTracking(taskID: String, then finished: @escaping @MainActor () -> Void = {}) -> Bool {
        guard availability.canStart(taskID: taskID) else { return false }
        let expectedActiveID = active?.id
        let occurredAt = formatTimestamp(clock.now)
        sendTracking(then: finished) { client in
            _ = try await client.startTracking(
                taskID: taskID, expectedActiveID: expectedActiveID, occurredAt: occurredAt)
        }
        return true
    }

    /// Stops the running worklog `worklogID` and calls `finished` once the request and its reload end. Does nothing
    /// and returns `false` when another worklog runs or `TrackingAvailability.canStop` is false.
    @discardableResult
    public func stopTracking(worklogID: String, then finished: @escaping @MainActor () -> Void = {}) -> Bool {
        guard availability.canStop, active?.id == worklogID else { return false }
        let occurredAt = formatTimestamp(clock.now)
        sendTracking(then: finished) { client in
            _ = try await client.stopTracking(worklogID: worklogID, occurredAt: occurredAt)
        }
        return true
    }

    /// Hides the tracking error.
    public func dismissError() { clearError() }

    private func sendTracking(
        then finished: @escaping @MainActor () -> Void,
        _ request: @escaping @MainActor (any TrackerClient) async throws -> Void
    ) {
        cancelAutoResume()
        clearError()
        beginSubmitting()
        Task { [weak self] in
            guard let self else { return }
            do {
                try await request(client)
            } catch {
                recordFailure(error)
            }
            await reloadTaskList()
            showRecordedError()
            endSubmitting()
            finished()
        }
    }

    /// Takes in the running worklog of a loaded task list, restarts the elapsed time when another worklog runs or
    /// its start changed, and shows the timer text. The connection is confirmed by now, so `TrackerSync` calls this
    /// after `ConnectionStore.acceptTaskList()`.
    func acceptTaskList(active worklog: WorklogItem?) {
        let previous = active
        set(\.active, worklog)
        if previous?.id != active?.id || previous?.start != active?.start {
            resetElapsedAnchor()
        }
        displayClock.setWorklogRunning(active != nil)
        refreshTimerText()
    }

    /// Measures the running worklog from the current time and shows the timer text. `TrackerSync` calls this on wake.
    func resetElapsedAnchor() {
        let now = clock.now
        timerBase = max(0, now.timeIntervalSince(parseTimestamp(active?.start) ?? now))
        timerAnchor = clock.uptime
        refreshTimerText()
    }

    func elapsed() -> TimeInterval? {
        guard active != nil else { return nil }
        return timerBase + max(0, clock.uptime - timerAnchor)
    }

    func displayText() -> String {
        guard connection.confirmed else { return "Unavailable" }
        return elapsed().map(formatClockDuration) ?? "Idle"
    }

    private func refreshTimerText() {
        set(\.timerText, displayText())
        set(\.elapsedSeconds, connection.confirmed ? elapsed().map { Int($0) } : nil)
    }

    func beginSubmitting() { set(\.isSubmitting, true) }

    func endSubmitting() { set(\.isSubmitting, false) }

    /// Records the failure of a tracking request. `showRecordedError()` shows it.
    func recordFailure(_ error: Error) { recordedError = writeFailure(error).localizedDescription }

    /// Shows the recorded failure, or hides the error when none was recorded. The flow that sent a tracking request
    /// calls this after its reload.
    func showRecordedError() { set(\.error, recordedError) }

    /// Drops the recorded failure and hides the error.
    func clearError() {
        recordedError = nil
        set(\.error, nil)
    }

    /// Drops the previous connection's tracking error.
    func acceptConnection() { clearError() }
}
