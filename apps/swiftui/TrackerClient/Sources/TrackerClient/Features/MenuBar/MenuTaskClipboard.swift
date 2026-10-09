import Foundation

public extension TrackerSession {
    func menuCopyValue(
        _ action: MenuShortcutAction, taskID: String,
        connection: ConnectionSettings
    ) -> String? {
        // A dropdown opened on one data source must not copy from another.
        guard connectionSettings == connection,
            let task = menuTask(withID: taskID)
        else { return nil }
        switch action {
        case .copyName: return task.name
        case .copyExact: return MenuDurationFormatter.exact(dailyDuration(taskID: taskID))
        case .copyRounded: return MenuDurationFormatter.rounded(dailyDuration(taskID: taskID))
        case .openMenu, .moveDown, .moveUp: return nil
        }
    }

    private func menuTask(withID taskID: String) -> TaskItem? {
        for task in tasks {
            if task.id == taskID { return task }
        }
        return nil
    }
}
