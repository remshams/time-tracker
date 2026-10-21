import Foundation

@MainActor
public final class TrackerMenuPresentationObserver {
    public var onLabelChange: (() -> Void)?
    public var onValuesChange: (() -> Void)?
    public private(set) var content: TrackerMenuContent
    public private(set) var label: TrackerMenuLabelContent
    public private(set) var values: TrackerMenuValues
    public private(set) var taskOrder: MenuTaskOrder
    private var menuOpen = false

    public init(session: TrackerSession, showDailyTotal: Bool, taskOrder: MenuTaskOrder = .newestFirst) {
        self.taskOrder = taskOrder
        content = TrackerMenuContent(session, taskOrder: taskOrder)
        label = TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal)
        values = TrackerMenuValues(session, content: content)
    }

    public func update(from session: TrackerSession, showDailyTotal: Bool) {
        // Keep navigation stable while time values continue to update.
        let nextContent = menuOpen ? content : TrackerMenuContent(session, taskOrder: taskOrder)
        let nextLabel = menuOpen ? label : TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal)
        let nextValues = TrackerMenuValues(session, content: nextContent)
        let labelChanged = label != nextLabel
        let valuesChanged = values != nextValues
        content = nextContent
        label = nextLabel
        values = nextValues
        if labelChanged { onLabelChange?() }
        if valuesChanged { onValuesChange?() }
    }

    /// An open menu keeps its captured order until it closes.
    public func setTaskOrder(_ order: MenuTaskOrder, session: TrackerSession, showDailyTotal: Bool) {
        taskOrder = order
        update(from: session, showDailyTotal: showDailyTotal)
    }

    public func menuOpened(from session: TrackerSession, showDailyTotal: Bool) {
        update(from: session, showDailyTotal: showDailyTotal)
        menuOpen = true
    }

    public func menuClosed(from session: TrackerSession, showDailyTotal: Bool) {
        menuOpen = false
        update(from: session, showDailyTotal: showDailyTotal)
    }
}
