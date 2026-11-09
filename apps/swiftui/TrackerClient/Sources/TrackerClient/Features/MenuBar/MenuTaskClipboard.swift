import Foundation

extension MenuBarStore {
    public func menuCopyValue(
        _ action: MenuShortcutAction, taskID: String,
        connection: ConnectionSettings
    ) -> String? {
        // A dropdown opened on one data source must not copy from another.
        guard self.connection.settings == connection,
            let menuTask = task(withID: taskID, in: catalog.tasks)
        else { return nil }
        switch action {
        case .copyName: return menuTask.name
        case .copyExact: return MenuDurationFormatter.exact(dailyDuration(taskID: taskID))
        case .copyRounded: return MenuDurationFormatter.rounded(dailyDuration(taskID: taskID))
        case .openMenu, .moveDown, .moveUp: return nil
        }
    }

    /// Computed from the clock on each read, so copying from the menu uses the current time.
    func dailyDuration(taskID: String) -> TimeInterval? {
        dailyTotals.duration(taskID: taskID, active: tracking.active, clock: clock)
    }
}
