import Foundation

@testable import App

enum TestFactories {
    static let anyTaskID: Task.ID = UUID(uuidString: "00000000-0000-0000-0000-000000000001")!

    static func makeTask(id: Task.ID = .init(), title: String, description: String? = nil) -> Task {
        // swiftlint:disable:next force_try
        try! Task(id: id, title: title, description: description)
    }

    static func makeWorkLogEntry(
        id: UUID = .init(),
        taskID: Task.ID,
        description: String? = nil,
        startedAt: Date = Date(timeIntervalSince1970: 1_700_000_000),
        addedAt: Date = Date(timeIntervalSince1970: 1_700_000_060),
        // Defaults to a completed entry so duration is non-nil in most fixture scenarios.
        endedAt: Date? = Date(timeIntervalSince1970: 1_700_000_360),
        updatedAt: Date = Date(timeIntervalSince1970: 1_700_000_420)
    ) -> WorkLogEntry {
        // swiftlint:disable:next force_try
        try! WorkLogEntry(
            id: id,
            taskID: taskID,
            description: description,
            startedAt: startedAt,
            addedAt: addedAt,
            endedAt: endedAt,
            updatedAt: updatedAt)
    }
}
