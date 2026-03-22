import Foundation
import Testing

@testable import App

@Test func workLogEntryStoresProvidedFields() {
  let task = TestFactories.makeTask(title: "Write project plan")
  let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
  let addedAt = Date(timeIntervalSince1970: 1_700_000_060)
  let endedAt = Date(timeIntervalSince1970: 1_700_000_360)
  let updatedAt = Date(timeIntervalSince1970: 1_700_000_420)

  let entry = TestFactories.makeWorkLogEntry(
    taskID: task.id,
    description: "Initial architecture and constraints",
    startedAt: startedAt,
    addedAt: addedAt,
    endedAt: endedAt,
    updatedAt: updatedAt)

  #expect(entry.taskID == task.id)
  #expect(entry.description == "Initial architecture and constraints")
  #expect(entry.startedAt == startedAt)
  #expect(entry.addedAt == addedAt)
  #expect(entry.endedAt == endedAt)
  #expect(entry.updatedAt == updatedAt)
}

@Test func workLogEntryKeepsExplicitIdentifier() {
  let identifier = UUID()

  let entry = TestFactories.makeWorkLogEntry(id: identifier, taskID: TestFactories.anyTaskID)

  #expect(entry.id == identifier)
}

@Test func workLogEntryAllowsMissingDescription() {
  let entry = TestFactories.makeWorkLogEntry(taskID: TestFactories.anyTaskID)

  #expect(entry.description == nil)
}

@Test func workLogEntryAllowsMissingEndedAt() {
  let entry = TestFactories.makeWorkLogEntry(taskID: TestFactories.anyTaskID, endedAt: nil)

  #expect(entry.endedAt == nil)
  #expect(entry.duration == nil)
}

@Test func workLogEntryComputesDurationFromStartAndEnd() {
  let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
  let endedAt = Date(timeIntervalSince1970: 1_700_000_900)

  let entry = TestFactories.makeWorkLogEntry(
    taskID: TestFactories.anyTaskID,
    startedAt: startedAt,
    endedAt: endedAt)

  #expect(entry.duration == .seconds(900))
}

@Test func workLogEntryAllowsEndedAtEqualToStartedAt() {
  let startedAt = Date(timeIntervalSince1970: 1_700_000_900)

  let entry = TestFactories.makeWorkLogEntry(
    taskID: TestFactories.anyTaskID,
    startedAt: startedAt,
    endedAt: startedAt)

  #expect(entry.duration == .seconds(0))
}

@Test func workLogEntryRejectsEndedAtEarlierThanStartedAt() {
  let startedAt = Date(timeIntervalSince1970: 1_700_000_900)
  let endedAt = Date(timeIntervalSince1970: 1_700_000_100)

  #expect(throws: WorkLogEntry.ValidationError.endedBeforeStarted) {
    try WorkLogEntry(taskID: TestFactories.anyTaskID, startedAt: startedAt, endedAt: endedAt)
  }
}

@Test func workLogEntryRejectsUpdatedAtEarlierThanAddedAt() {
  let startedAt = Date(timeIntervalSince1970: 1_700_000_100)
  let addedAt = Date(timeIntervalSince1970: 1_700_000_300)
  let updatedAt = Date(timeIntervalSince1970: 1_700_000_200)

  #expect(throws: WorkLogEntry.ValidationError.updatedBeforeAdded) {
    try WorkLogEntry(
      taskID: TestFactories.anyTaskID,
      startedAt: startedAt,
      addedAt: addedAt,
      updatedAt: updatedAt)
  }
}

@Test func workLogEntryAllowsUpdatedAtEqualToAddedAt() {
  let startedAt = Date(timeIntervalSince1970: 1_700_000_100)
  let addedAt = Date(timeIntervalSince1970: 1_700_000_300)

  let entry = TestFactories.makeWorkLogEntry(
    taskID: TestFactories.anyTaskID,
    startedAt: startedAt,
    addedAt: addedAt,
    updatedAt: addedAt)

  #expect(entry.addedAt == addedAt)
  #expect(entry.updatedAt == addedAt)
}

@Test func workLogEntryUsesValueEquality() {
  let id = UUID()
  let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
  let addedAt = Date(timeIntervalSince1970: 1_700_000_060)
  let endedAt = Date(timeIntervalSince1970: 1_700_000_360)
  let updatedAt = Date(timeIntervalSince1970: 1_700_000_420)

  let firstEntry = TestFactories.makeWorkLogEntry(
    id: id,
    taskID: TestFactories.anyTaskID,
    description: "Initial architecture and constraints",
    startedAt: startedAt,
    addedAt: addedAt,
    endedAt: endedAt,
    updatedAt: updatedAt)
  let secondEntry = TestFactories.makeWorkLogEntry(
    id: id,
    taskID: TestFactories.anyTaskID,
    description: "Initial architecture and constraints",
    startedAt: startedAt,
    addedAt: addedAt,
    endedAt: endedAt,
    updatedAt: updatedAt)

  #expect(firstEntry == secondEntry)
}
