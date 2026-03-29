import Foundation
import Testing

@testable import TimeTracker

@Suite struct InMemoryWorkLogRepositoryTests {
  @Test func returnsSeededEntriesForTaskID() async throws {
    let taskA: WorkTask = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let taskB: WorkTask = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let entryA1 = TestFactories.makeWorkLogEntry(taskID: taskA.id, description: "Initial architecture and constraints")
    let entryA2 = TestFactories.makeWorkLogEntry(taskID: taskA.id, description: "Draft implementation plan")
    let entryB1 = TestFactories.makeWorkLogEntry(taskID: taskB.id, description: "Review assumptions")
    let repository = InMemoryWorkLogRepository(entriesByTaskID: [
      taskA.id: [entryA1, entryA2],
      taskB.id: [entryB1],
    ])

    let fetchedEntries = try await repository.fetchEntries(for: taskA.id)

    #expect(fetchedEntries == [entryA1, entryA2])
  }

  @Test func returnsAnEmptyListByDefault() async throws {
    let repository = InMemoryWorkLogRepository()

    let fetchedEntries = try await repository.fetchEntries(for: UUID())

    #expect(fetchedEntries.isEmpty)
  }

  @Test func returnsAnEmptyListForUnknownTaskID() async throws {
    let taskA: WorkTask = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let taskB: WorkTask = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
    let entryA1 = TestFactories.makeWorkLogEntry(taskID: taskA.id, description: "Initial architecture and constraints")
    let repository = InMemoryWorkLogRepository(entriesByTaskID: [taskA.id: [entryA1]])

    let fetchedEntries = try await repository.fetchEntries(for: taskB.id)

    #expect(fetchedEntries.isEmpty)
  }
}
