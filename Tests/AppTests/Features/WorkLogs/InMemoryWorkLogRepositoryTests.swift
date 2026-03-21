import Foundation
import Testing

@testable import App

@Test func inMemoryWorkLogRepositoryReturnsSeededEntriesForTaskID() async throws {
    let taskA = makeTask(title: "Write project plan")
    let taskB = makeTask(title: "Review next step")
    let entryA1 = makeWorkLogEntry(taskID: taskA.id, description: "Initial architecture and constraints")
    let entryA2 = makeWorkLogEntry(taskID: taskA.id, description: "Draft implementation plan")
    let entryB1 = makeWorkLogEntry(taskID: taskB.id, description: "Review assumptions")
    let repository = InMemoryWorkLogRepository(entriesByTaskID: [
        taskA.id: [entryA1, entryA2],
        taskB.id: [entryB1],
    ])

    let fetchedEntries = try await repository.fetchEntries(for: taskA.id)

    #expect(fetchedEntries == [entryA1, entryA2])
}

@Test func inMemoryWorkLogRepositoryReturnsAnEmptyListByDefault() async throws {
    let repository = InMemoryWorkLogRepository()

    let fetchedEntries = try await repository.fetchEntries(for: UUID())

    #expect(fetchedEntries.isEmpty)
}

@Test func inMemoryWorkLogRepositoryReturnsAnEmptyListForUnknownTaskID() async throws {
    let taskA = makeTask(title: "Write project plan")
    let taskB = makeTask(title: "Review next step")
    let entryA1 = makeWorkLogEntry(taskID: taskA.id, description: "Initial architecture and constraints")
    let repository = InMemoryWorkLogRepository(entriesByTaskID: [taskA.id: [entryA1]])

    let fetchedEntries = try await repository.fetchEntries(for: taskB.id)

    #expect(fetchedEntries.isEmpty)
}

private func makeTask(id: Task.ID = .init(), title: String, description: String? = nil) -> Task {
    do {
        return try Task(id: id, title: title, description: description)
    } catch {
        Issue.record("Failed to create test task: \(error)")
        fatalError("Failed to create test task: \(error)")
    }
}

private func makeWorkLogEntry(
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
            updatedAt: updatedAt
        )
    } catch {
        Issue.record("Failed to create test work log entry: \(error)")
        fatalError("Failed to create test work log entry: \(error)")
    }
}
