import Foundation
import Testing

@testable import TimeTracker

@Suite struct InMemoryWorkLogRepositoryTests {
  @Suite @MainActor struct FetchEntries {
    @Test func returnsSeededEntriesForTaskID() async throws {
      let taskA: WorkTask = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let taskB: WorkTask = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let entryA1 = TestFactories.makeWorkLogEntry(
        taskID: taskA.id,
        description: "Initial architecture and constraints")
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
      let entryA1 = TestFactories.makeWorkLogEntry(
        taskID: taskA.id,
        description: "Initial architecture and constraints")
      let repository = InMemoryWorkLogRepository(entriesByTaskID: [taskA.id: [entryA1]])

      let fetchedEntries = try await repository.fetchEntries(for: taskB.id)

      #expect(fetchedEntries.isEmpty)
    }
  }

  @Suite @MainActor struct AddEntry {
    @Test func addedEntryAppearsInSubsequentFetch() async throws {
      let taskID = TestFactories.anyTaskID
      let entry = TestFactories.makeWorkLogEntry(taskID: taskID)
      let repository = InMemoryWorkLogRepository()

      try await repository.addEntry(entry)
      let fetchedEntries = try await repository.fetchEntries(for: taskID)

      #expect(fetchedEntries == [entry])
    }

    @Test func addEntryDoesNotAffectOtherTaskIDs() async throws {
      let taskA = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let taskB = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let existingEntry = TestFactories.makeWorkLogEntry(taskID: taskA.id)
      let repository = InMemoryWorkLogRepository(entriesByTaskID: [taskA.id: [existingEntry]])
      let newEntry = TestFactories.makeWorkLogEntry(taskID: taskB.id)

      try await repository.addEntry(newEntry)
      let taskAEntries = try await repository.fetchEntries(for: taskA.id)

      #expect(taskAEntries == [existingEntry])
    }

    @Test func addMultipleEntriesForSameTaskID() async throws {
      let taskID = TestFactories.anyTaskID
      let entryOne = TestFactories.makeWorkLogEntry(taskID: taskID, description: "First")
      let entryTwo = TestFactories.makeWorkLogEntry(taskID: taskID, description: "Second")
      let repository = InMemoryWorkLogRepository()

      try await repository.addEntry(entryOne)
      try await repository.addEntry(entryTwo)
      let fetchedEntries = try await repository.fetchEntries(for: taskID)

      #expect(fetchedEntries == [entryOne, entryTwo])
    }

  }

  @Suite @MainActor struct UpdateEntry {
    @Test func updatedEntryReplacesOriginalInStore() async throws {
      let taskID = TestFactories.anyTaskID
      let original = TestFactories.makeRunningWorkLogEntry(taskID: taskID)
      let repository = InMemoryWorkLogRepository(entriesByTaskID: [taskID: [original]])
      let endedAt = Date(timeIntervalSince1970: 1_700_000_900)
      let updated = TestFactories.makeWorkLogEntry(
        id: original.id,
        taskID: taskID,
        startedAt: original.startedAt,
        addedAt: original.addedAt,
        endedAt: endedAt,
        updatedAt: endedAt)

      try await repository.updateEntry(updated)
      let fetchedEntries = try await repository.fetchEntries(for: taskID)

      #expect(fetchedEntries == [updated])
    }

    @Test func updateEntryThrowsWhenEntryNotFound() async throws {
      let repository = InMemoryWorkLogRepository()
      let unknownEntry = TestFactories.makeWorkLogEntry(taskID: TestFactories.anyTaskID)

      await #expect(throws: WorkLogRepositoryError.entryNotFound) {
        try await repository.updateEntry(unknownEntry)
      }
    }
  }

  @Suite @MainActor struct FetchRunningEntry {
    @Test func returnsRunningEntryAcrossAllTasks() async throws {
      let taskA = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let taskB = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let completedEntry = TestFactories.makeWorkLogEntry(taskID: taskA.id)
      let runningEntry = TestFactories.makeRunningWorkLogEntry(taskID: taskB.id)
      let repository = InMemoryWorkLogRepository(entriesByTaskID: [
        taskA.id: [completedEntry],
        taskB.id: [runningEntry],
      ])

      let result = try await repository.fetchRunningEntry()

      #expect(result == runningEntry)
    }

    @Test func returnsNilWhenNoRunningEntryExists() async throws {
      let taskID = TestFactories.anyTaskID
      let completedEntry = TestFactories.makeWorkLogEntry(taskID: taskID)
      let repository = InMemoryWorkLogRepository(entriesByTaskID: [taskID: [completedEntry]])

      let result = try await repository.fetchRunningEntry()

      #expect(result == nil)
    }

    @Test func returnsNilForEmptyRepository() async throws {
      let repository = InMemoryWorkLogRepository()

      let result = try await repository.fetchRunningEntry()

      #expect(result == nil)
    }
  }
}
