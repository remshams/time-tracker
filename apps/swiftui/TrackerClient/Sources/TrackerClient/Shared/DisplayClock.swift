import Foundation

/// Ticks while the app shows a running worklog's time, so the stores can refresh the values they derive from the
/// clock.
///
/// It ticks every second while a window or the menu is visible, and every minute while the menu bar shows today's
/// total. Both run only while a worklog runs and the Mac is awake. `TrackerSync` reports whether the app is awake and
/// visible, `TrackingStore` whether a worklog runs, and `MenuBarStore` whether the menu bar shows the total. The stores
/// that show clock values register their refresh with `onTick`.
final class DisplayClock {
    private let scheduler: any TrackerScheduler
    private var refreshes: [@MainActor () -> Void] = []
    private var awake = false
    private var visible = false
    private var menuBarTotalShown = false
    private var worklogRunning = false
    // The scheduler runs no callback after cancel(), so the callbacks need no staleness check.
    private var secondTimer: (any TrackerCancellation)?
    private var minuteTimer: (any TrackerCancellation)?

    init(scheduler: any TrackerScheduler) { self.scheduler = scheduler }

    isolated deinit {
        secondTimer?.cancel()
        minuteTimer?.cancel()
    }

    /// Runs `refresh` on each tick, and when a window or the menu becomes visible or hidden.
    func onTick(_ refresh: @escaping @MainActor () -> Void) { refreshes.append(refresh) }

    /// Whether the sync has started and the Mac is awake.
    func setAwake(_ awake: Bool) {
        self.awake = awake
        updateTimers()
    }

    /// Whether a window or the menu is visible. A change refreshes the values at once, because they may be stale
    /// while nothing showed them.
    func setVisible(_ visible: Bool) {
        guard self.visible != visible else { return }
        self.visible = visible
        updateTimers()
        tick()
    }

    /// Whether the menu bar shows today's total.
    func setMenuBarTotalShown(_ shown: Bool) {
        menuBarTotalShown = shown
        updateTimers()
    }

    /// Whether a worklog runs.
    func setWorklogRunning(_ running: Bool) {
        worklogRunning = running
        updateTimers()
    }

    private func tick() {
        for refresh in refreshes { refresh() }
    }

    private func updateTimers() {
        let ticking = awake && worklogRunning
        if ticking && visible {
            secondTimer = secondTimer ?? schedule(every: 1, tolerance: 0.2)
        } else {
            secondTimer?.cancel()
            secondTimer = nil
        }
        if ticking && menuBarTotalShown {
            minuteTimer = minuteTimer ?? schedule(every: 60, tolerance: 5)
        } else {
            minuteTimer?.cancel()
            minuteTimer = nil
        }
    }

    private func schedule(every interval: TimeInterval, tolerance: TimeInterval) -> any TrackerCancellation {
        scheduler.schedule(after: interval, repeating: true, tolerance: tolerance) { [weak self] in self?.tick() }
    }
}
