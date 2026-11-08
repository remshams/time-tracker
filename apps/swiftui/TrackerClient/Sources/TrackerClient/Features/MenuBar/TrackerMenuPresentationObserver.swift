import Foundation
import Observation

/// Builds the menu bar's content, label and live values from the stores.
///
/// One `Observations` loop reads every value the menu shows and rebuilds the menu after each change. While
/// the menu is open, its content and label stay as they were when it opened, and only the time values update.
public final class TrackerMenuPresentationObserver {
    public var onLabelChange: (() -> Void)?
    public var onValuesChange: (() -> Void)?
    public private(set) var content: TrackerMenuContent
    public private(set) var label: TrackerMenuLabelContent
    public private(set) var values: TrackerMenuValues
    public private(set) var taskOrder: MenuTaskOrder
    public private(set) var showDailyTotal: Bool
    /// The store values of the last rebuild. Tests wait until it matches the stores.
    private(set) var lastInputs: TrackerMenuInputs
    private let sources: TrackerMenuSources
    private var menuOpen = false

    public init(sources: TrackerMenuSources, showDailyTotal: Bool, taskOrder: MenuTaskOrder = .newestFirst) {
        self.sources = sources
        self.showDailyTotal = showDailyTotal
        self.taskOrder = taskOrder
        let inputs = sources.read()
        lastInputs = inputs
        content = TrackerMenuContent(inputs, taskOrder: taskOrder)
        label = TrackerMenuLabelContent(inputs, showDailyTotal: showDailyTotal)
        values = TrackerMenuValues(inputs, content: content)
        // The closure reads the same inputs whether or not the menu is open. A start or stop check skips a store
        // only while an earlier condition already decides the result, and a change to that condition reads it again.
        // The loop ends at the first change after the observer is released.
        Task { [weak self, sources] in
            for await inputs in Observations({ sources.read() }) {
                guard let self else { return }
                update(inputs)
            }
        }
    }

    private func update(_ inputs: TrackerMenuInputs) {
        lastInputs = inputs
        // Keep navigation stable while time values continue to update.
        let nextContent = menuOpen ? content : TrackerMenuContent(inputs, taskOrder: taskOrder)
        let nextLabel = menuOpen ? label : TrackerMenuLabelContent(inputs, showDailyTotal: showDailyTotal)
        let nextValues = TrackerMenuValues(inputs, content: nextContent)
        let labelChanged = label != nextLabel
        let valuesChanged = values != nextValues
        content = nextContent
        label = nextLabel
        values = nextValues
        if labelChanged { onLabelChange?() }
        if valuesChanged { onValuesChange?() }
    }

    private func rebuild() { update(sources.read()) }

    public func setShowDailyTotal(_ enabled: Bool) {
        showDailyTotal = enabled
        rebuild()
    }

    /// An open menu keeps its captured order until it closes.
    public func setTaskOrder(_ order: MenuTaskOrder) {
        taskOrder = order
        rebuild()
    }

    /// Captures the content from the current store values. Publish the stores first.
    public func menuOpened() {
        rebuild()
        menuOpen = true
    }

    public func menuClosed() {
        menuOpen = false
        rebuild()
    }
}
