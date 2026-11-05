import Foundation
import Observation

/// Holds the task list, the Active or Archived tab, and the selected task for the whole app.
///
/// Views read the properties; `TrackerSync` and `TrackerSession` write them. Each property is assigned only
/// when its value changes, so an unchanged poll redraws nothing and a change to another task leaves the
/// selected task's readers alone.
@Observable
public final class TaskCatalogStore: ChangeGuardedStore {
    public private(set) var tasks: [TaskItem] = []
    public private(set) var selectedTaskID: String?
    public private(set) var tab: TaskTab = .active
    public private(set) var visibleTasks: [TaskItem] = []
    public private(set) var selectedTask: TaskItem?
    // Each tab's last selection, restored when the user switches back to that tab.
    @ObservationIgnored private var activeSelection: String?
    @ObservationIgnored private var archivedSelection: String?

    func changeTab(_ newTab: TaskTab) -> Bool {
        guard tab != newTab else { return false }
        tab = newTab
        set(\.selectedTaskID, selection(for: newTab) ?? firstTaskID(in: newTab))
        rememberSelection()
        updateDerivedValues()
        return true
    }

    func select(_ taskID: String?) -> Bool {
        guard let taskID, matchingTaskID(taskID, in: tab) != nil, selectedTaskID != taskID else {
            return false
        }
        selectedTaskID = taskID
        rememberSelection()
        updateDerivedValues()
        return true
    }

    func rememberRestoredTask(_ taskID: String) {
        guard tasks.contains(where: { $0.id == taskID && !$0.archived }) else { return }
        activeSelection = taskID
    }

    func resetSelections() {
        activeSelection = nil
        archivedSelection = nil
        set(\.selectedTaskID, nil)
        updateDerivedValues()
    }

    func apply(_ newTasks: [TaskItem], previousActive: WorklogItem?, active: WorklogItem?) -> Bool {
        let previousTask = selectedTaskID
        let previousLatest = selectedTask?.latestStart
        set(\.tasks, newTasks)
        if selectedTaskID.flatMap({ matchingTaskID($0, in: tab) }) == nil {
            set(\.selectedTaskID, firstTaskID(in: tab))
        }
        rememberSelection()
        updateDerivedValues()
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

    private func updateDerivedValues() {
        set(\.visibleTasks, tasks.filter { $0.archived == (tab == .archived) })
        set(\.selectedTask, tasks.first { $0.id == selectedTaskID })
    }
}
