import Foundation
import Testing

@testable import App

enum TestFactories {
    static func makeTask(id: Task.ID = .init(), title: String, description: String? = nil) -> Task {
        do {
            return try Task(id: id, title: title, description: description)
        } catch {
            Issue.record("Failed to create test task: \(error)")
            fatalError("Failed to create test task: \(error)")
        }
    }

    static func makeWorkLogEntry(
        id: UUID = .init(),
        taskID: Task.ID,
        description: String? = nil,
        startedAt: Date = Date(timeIntervalSince1970: 1_700_000_000),
        addedAt: Date = Date(timeIntervalSince1970: 1_700_000_060),
        endedAt: Date? = Date(timeIntervalSince1970: 1_700_000_360),
        updatedAt: Date = Date(timeIntervalSince1970: 1_700_000_420)
    ) -> WorkLogEntry {
        do {
            return try WorkLogEntry(
                id: id,
                taskID: taskID,
                description: description,
                startedAt: startedAt,
                addedAt: addedAt,
                endedAt: endedAt,
                updatedAt: updatedAt)
        } catch {
            Issue.record("Failed to create test work log entry: \(error)")
            fatalError("Failed to create test work log entry: \(error)")
        }
    }
}
