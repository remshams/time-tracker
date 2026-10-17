import Foundation

/// The blocking rules of one feature editor. Each editor states its own rules,
/// so adding an editor does not change the gates of the others.
@MainActor
protocol TrackerEditor: AnyObject {
    /// A submitted write waits for the operation gate, so tracking controls stay disabled.
    var blocksControls: Bool { get }
    /// Presented or unresolved work keeps every other editor closed.
    var blocksOtherEditors: Bool { get }
    /// The reason a connection change must wait for this editor, or nil.
    var connectionChangeBlocker: String? { get }
    /// A submitted write resumes when the computer wakes.
    var hasPendingWrite: Bool { get }
    func sleep()
    func connectionDidChange()
    func observeActive(_ worklog: WorklogItem)
    func observeHistory(_ worklog: WorklogItem)
    func reset()
}

extension TrackerEditor {
    func sleep() {}
    func connectionDidChange() {}
    func observeActive(_ worklog: WorklogItem) {}
    func observeHistory(_ worklog: WorklogItem) {}
}

/// A feature workflow that owns one editor and its operations.
@MainActor
protocol EditorWorkflow: AnyObject {
    var editor: any TrackerEditor { get }
    /// Starts the editor's pending write. Returns false when nothing started.
    func drainWrite() -> Bool
    /// Starts a pending read, such as a search, that leaves tracking controls enabled.
    func drainRead() -> Bool
}

extension EditorWorkflow {
    func drainRead() -> Bool { false }
}

/// Answers the session's editor gates and drains editor operations from every registered workflow.
@MainActor
final class EditorRegistry {
    private let workflows: [any EditorWorkflow]
    private let editors: [any TrackerEditor]

    /// Registration order sets the drain order and which blocker a rejected connection change reports.
    init(_ workflows: [any EditorWorkflow]) {
        self.workflows = workflows
        editors = workflows.map { $0.editor }
    }

    var blocksControls: Bool { editors.contains { $0.blocksControls } }
    var hasPendingWrite: Bool { editors.contains { $0.hasPendingWrite } }
    var connectionChangeBlocker: String? { editors.lazy.compactMap { $0.connectionChangeBlocker }.first }
    var blocksConnectionChange: Bool { connectionChangeBlocker != nil }

    /// Whether an editor other than `editor` keeps the others closed.
    func blocksEditors(except editor: (any TrackerEditor)?) -> Bool {
        editors.contains { $0 !== editor && $0.blocksOtherEditors }
    }

    /// Starts the first pending write in registration order.
    func drainWrite() -> Bool {
        for workflow in workflows {
            if workflow.drainWrite() { return true }
        }
        return false
    }

    /// Starts the first pending read in registration order.
    func drainRead() -> Bool {
        for workflow in workflows {
            if workflow.drainRead() { return true }
        }
        return false
    }

    func sleep() { editors.forEach { $0.sleep() } }
    func connectionDidChange() { editors.forEach { $0.connectionDidChange() } }
    func observeActive(_ worklog: WorklogItem) { editors.forEach { $0.observeActive(worklog) } }
    func observeHistory(_ worklog: WorklogItem) { editors.forEach { $0.observeHistory(worklog) } }
    func reset() { editors.forEach { $0.reset() } }
}
