import Foundation
import Observation

/// Holds the task list in the chosen order, the Active or Archived tab, and the selected task for the whole app.
///
/// Views read the properties and change the order, tab and selection; `TrackerSync` and the dialog stores' follow-ups
/// write them too. Each property is assigned only when its value changes, so an unchanged poll redraws nothing and a
/// change to another task leaves the selected task's readers alone.
@Observable
public final class TaskCatalogStore: ChangeGuardedStore {
    /// The loaded tasks, sorted by `taskOrder`. The sidebar and the menu bar show them in this order.
    public private(set) var tasks: [TaskItem] = []
    public private(set) var taskOrder: TaskOrder
    public private(set) var selectedTaskID: String?
    public private(set) var tab: TaskTab = .active
    public private(set) var visibleTasks: [TaskItem] = []
    public private(set) var selectedTask: TaskItem?
    // Each tab's last selection, restored when the user switches back to that tab.
    @ObservationIgnored private var activeSelection: String?
    @ObservationIgnored private var archivedSelection: String?
    /// Loads the selected task's history after the user changed the tab or the selection. `TrackerFeatures` sets it
    /// once, after it creates the history store, which needs this store.
    @ObservationIgnored var reloadHistory: @MainActor () -> Void = {}
    private let orderRepository: (any TaskOrderRepository)?

    /// Starts with the saved order, or the default order without a repository.
    init(orderRepository: (any TaskOrderRepository)? = nil) {
        self.orderRepository = orderRepository
        taskOrder = orderRepository?.load() ?? .newestFirst
    }

    /// Sorts the tasks by `order` and saves it. The selection stays.
    public func setTaskOrder(_ order: TaskOrder) {
        guard taskOrder != order else { return }
        taskOrder = order
        orderRepository?.save(order)
        set(\.tasks, order.sorted(tasks))
        updateDerivedValues()
    }

    /// Switches to `newTab` and reloads the history when the selection changes with it.
    public func changeTab(_ newTab: TaskTab) {
        if switchTab(to: newTab) { reloadHistory() }
    }

    /// Selects `taskID`, or no task, and reloads the history when the selection changed.
    public func select(_ taskID: String?) {
        if switchSelection(to: taskID) { reloadHistory() }
    }

    /// Changes to the tab and selection of a task the tracker just created, and reloads the history once if either
    /// changed. The reload before this call may have failed. The catalog then doesn't hold the task, so `created`
    /// decides the tab, and that tab restores its remembered selection or its first task.
    func showTask(_ created: TaskItem) {
        let confirmed = tasks.first { $0.id == created.id } ?? created
        let changedTab = switchTab(to: confirmed.archived ? .archived : .active)
        let changedSelection = switchSelection(to: created.id)
        if changedTab || changedSelection { reloadHistory() }
    }

    /// Changes the tab without loading the history. Returns whether the tab changed.
    func switchTab(to newTab: TaskTab) -> Bool {
        guard tab != newTab else { return false }
        tab = newTab
        set(\.selectedTaskID, selection(for: newTab) ?? firstTaskID(in: newTab))
        rememberSelection()
        updateDerivedValues()
        return true
    }

    /// Changes the selection without loading the history. Returns whether the selection changed.
    func switchSelection(to taskID: String?) -> Bool {
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

    /// Forgets both tabs' selections for a new connection. The tab stays.
    func acceptConnection() {
        activeSelection = nil
        archivedSelection = nil
        set(\.selectedTaskID, nil)
        updateDerivedValues()
    }

    /// Takes in the tasks of a loaded task list and keeps the selection while it exists in the tab. Returns whether the
    /// selected task's history may have changed: the selection, its latest start, or its running worklog before
    /// (`previousActive`) and after (`active`) the update.
    func acceptTaskList(_ newTasks: [TaskItem], previousActive: WorklogItem?, active: WorklogItem?) -> Bool {
        let previousTask = selectedTaskID
        let previousLatest = selectedTask?.latestStart
        set(\.tasks, taskOrder.sorted(newTasks))
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
