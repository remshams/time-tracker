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

/// Answers the session's editor gates from every registered editor.
@MainActor
final class EditorRegistry {
    private let editors: [any TrackerEditor]

    /// Registration order decides which blocker a rejected connection change reports.
    init(_ editors: [any TrackerEditor]) {
        self.editors = editors
    }

    var blocksControls: Bool { editors.contains { $0.blocksControls } }
    var hasPendingWrite: Bool { editors.contains { $0.hasPendingWrite } }
    var connectionChangeBlocker: String? { editors.lazy.compactMap { $0.connectionChangeBlocker }.first }
    var blocksConnectionChange: Bool { connectionChangeBlocker != nil }

    /// Whether an editor other than `editor` keeps the others closed.
    func blocksEditors(except editor: (any TrackerEditor)?) -> Bool {
        editors.contains { $0 !== editor && $0.blocksOtherEditors }
    }

    func sleep() { editors.forEach { $0.sleep() } }
    func connectionDidChange() { editors.forEach { $0.connectionDidChange() } }
    func observeActive(_ worklog: WorklogItem) { editors.forEach { $0.observeActive(worklog) } }
    func observeHistory(_ worklog: WorklogItem) { editors.forEach { $0.observeHistory(worklog) } }
    func reset() { editors.forEach { $0.reset() } }
}
