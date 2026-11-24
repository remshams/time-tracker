import Foundation

/// What a copy action puts on the pasteboard for a task.
public enum TaskCopyAction: String, CaseIterable, Sendable {
    case copyName, copyExact, copyRounded

    public var title: String {
        switch self {
        case .copyName: return "Copy task name"
        case .copyExact: return "Copy exact duration"
        case .copyRounded: return "Copy rounded duration"
        }
    }
}
