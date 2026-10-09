import Foundation

@MainActor
final class TaskCatalogState {
    private(set) var tasks: [TaskItem] = []
    private(set) var selectedTaskID: String?
    private(set) var tab: TaskTab = .active
    private var activeSelection: String?
    private var archivedSelection: String?

    var visibleTasks: [TaskItem] { tasks.filter { $0.archived == (tab == .archived) } }
    var selectedTask: TaskItem? { tasks.first { $0.id == selectedTaskID } }

    func changeTab(_ newTab: TaskTab) -> Bool {
        guard tab != newTab else { return false }
        tab = newTab
        selectedTaskID = selection(for: newTab) ?? firstTaskID(in: newTab)
        rememberSelection()
        return true
    }

    func select(_ taskID: String?) -> Bool {
        guard let taskID, hasTask(withID: taskID, in: visibleTasks), selectedTaskID != taskID else {
            return false
        }
        selectedTaskID = taskID
        rememberSelection()
        return true
    }

    func rememberRestoredTask(_ taskID: String) {
        guard tasks.contains(where: { $0.id == taskID && !$0.archived }) else { return }
        activeSelection = taskID
    }

    func resetSelections() {
        activeSelection = nil
        archivedSelection = nil
        selectedTaskID = nil
    }

    func apply(_ newTasks: [TaskItem], previousActive: WorklogItem?, active: WorklogItem?) -> Bool {
        let previousTask = selectedTaskID
        let previousLatest = selectedTask?.latestStart
        if tasks != newTasks { tasks = newTasks }
        if let selectedTaskID, !hasTask(withID: selectedTaskID, in: visibleTasks) {
            self.selectedTaskID = firstTaskID(in: tab)
        } else if selectedTaskID == nil {
            selectedTaskID = firstTaskID(in: tab)
        }
        rememberSelection()
        let selectedActiveChanged =
            (previousActive?.taskId == selectedTaskID || active?.taskId == selectedTaskID)
            && (previousActive?.id != active?.id || previousActive?.start != active?.start)
        return previousTask != selectedTaskID || previousLatest != selectedTask?.latestStart || selectedActiveChanged
    }

    private func selection(for tab: TaskTab) -> String? {
        let saved = tab == .active ? activeSelection : archivedSelection
        guard let saved else { return nil }
        return matchingTaskID(saved, in: tab)
    }

    private func hasTask(withID id: String, in tasks: [TaskItem]) -> Bool {
        for task in tasks {
            if task.id == id { return true }
        }
        return false
    }

    private func matchingTaskID(_ id: String, in tab: TaskTab) -> String? {
        for task in tasks {
            if task.id == id && task.archived == (tab == .archived) { return task.id }
        }
        return nil
    }

    private func firstTaskID(in tab: TaskTab) -> String? {
        tasks.first { $0.archived == (tab == .archived) }?.id
    }

    private func rememberSelection() {
        if tab == .active { activeSelection = selectedTaskID } else { archivedSelection = selectedTaskID }
    }
}
