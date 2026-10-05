import Foundation

public enum TaskColor: String, CaseIterable, Sendable {
    case blue, teal, green, orange, red, purple, pink

    public static func forTaskID(_ taskID: String) -> TaskColor {
        // Swift's Hasher uses a random seed. FNV-1a keeps colors stable on every device.
        var hash: UInt64 = 14_695_981_039_346_656_037
        for byte in taskID.utf8 {
            hash = (hash ^ UInt64(byte)) &* 1_099_511_628_211
        }
        return allCases[Int(hash % UInt64(allCases.count))]
    }
}

public struct TaskIndicator: Equatable, Sendable {
    public let taskID: String?
    public let isRunning: Bool
    public let isStale: Bool
    public var color: TaskColor? { taskID.map(TaskColor.forTaskID) }
    public var symbol: String { isRunning && !isStale ? "circle.fill" : "circle" }

    public init(taskID: String?, isRunning: Bool, isStale: Bool = false) {
        self.taskID = taskID
        self.isRunning = isRunning && !isStale
        self.isStale = isStale
    }
}
