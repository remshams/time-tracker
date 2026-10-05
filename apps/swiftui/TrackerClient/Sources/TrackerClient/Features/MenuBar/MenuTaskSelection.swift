import Foundation

public struct MenuTaskSelection: Equatable, Sendable {
    public let taskIDs: [String]
    public private(set) var selectedTaskID: String?

    public init(taskIDs: [String], initialTaskID: String? = nil) {
        var seen: Set<String> = []
        self.taskIDs = taskIDs.filter { seen.insert($0).inserted }
        selectedTaskID = initialTaskID.flatMap { self.taskIDs.contains($0) ? $0 : nil } ?? self.taskIDs.first
    }

    public mutating func select(taskID: String) {
        guard taskIDs.contains(taskID) else { return }
        selectedTaskID = taskID
    }

    public mutating func moveDown() {
        guard let selectedTaskID, let index = taskIDs.firstIndex(of: selectedTaskID), index + 1 < taskIDs.count else { return }
        self.selectedTaskID = taskIDs[index + 1]
    }

    public mutating func moveUp() {
        guard let selectedTaskID, let index = taskIDs.firstIndex(of: selectedTaskID), index > 0 else { return }
        self.selectedTaskID = taskIDs[index - 1]
    }
}
