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
    let task = TestFactories.makeTask(title: "Write project plan")
    let identifier = UUID()

    let entry = TestFactories.makeWorkLogEntry(id: identifier, taskID: task.id)

    #expect(entry.id == identifier)
}

@Test func workLogEntryAllowsMissingDescription() {
    let task = TestFactories.makeTask(title: "Write project plan")

    let entry = TestFactories.makeWorkLogEntry(taskID: task.id)

    #expect(entry.description == nil)
}

@Test func workLogEntryAllowsMissingEndedAt() {
    let task = TestFactories.makeTask(title: "Write project plan")

    let entry = TestFactories.makeWorkLogEntry(taskID: task.id, endedAt: nil)

    #expect(entry.endedAt == nil)
    #expect(entry.duration == nil)
}

@Test func workLogEntryComputesDurationFromStartAndEnd() {
    let task = TestFactories.makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_900)

    let entry = TestFactories.makeWorkLogEntry(taskID: task.id, startedAt: startedAt, endedAt: endedAt)

    #expect(entry.duration == .seconds(900))
}

@Test func workLogEntryAllowsEndedAtEqualToStartedAt() {
    let task = TestFactories.makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_900)

    let entry = TestFactories.makeWorkLogEntry(taskID: task.id, startedAt: startedAt, endedAt: startedAt)

    #expect(entry.duration == .seconds(0))
}

@Test func workLogEntryRejectsEndedAtEarlierThanStartedAt() {
    let task = TestFactories.makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_900)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_100)

    #expect(throws: WorkLogEntry.ValidationError.endedBeforeStarted) {
        try WorkLogEntry(taskID: task.id, startedAt: startedAt, endedAt: endedAt)
    }
}

@Test func workLogEntryRejectsUpdatedAtEarlierThanAddedAt() {
    let task = TestFactories.makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_100)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_300)
    let updatedAt = Date(timeIntervalSince1970: 1_700_000_200)

    #expect(throws: WorkLogEntry.ValidationError.updatedBeforeAdded) {
        try WorkLogEntry(taskID: task.id, startedAt: startedAt, addedAt: addedAt, updatedAt: updatedAt)
    }
}

@Test func workLogEntryAllowsUpdatedAtEqualToAddedAt() {
    let task = TestFactories.makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_100)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_300)

    let entry = TestFactories.makeWorkLogEntry(
        taskID: task.id,
        startedAt: startedAt,
        addedAt: addedAt,
        updatedAt: addedAt)

    #expect(entry.addedAt == addedAt)
    #expect(entry.updatedAt == addedAt)
}

@Test func workLogEntryUsesValueEquality() {
    let task = TestFactories.makeTask(title: "Write project plan")
    let id = UUID()
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_060)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_360)
    let updatedAt = Date(timeIntervalSince1970: 1_700_000_420)

    let firstEntry = TestFactories.makeWorkLogEntry(
        id: id,
        taskID: task.id,
        description: "Initial architecture and constraints",
        startedAt: startedAt,
        addedAt: addedAt,
        endedAt: endedAt,
        updatedAt: updatedAt)
    let secondEntry = TestFactories.makeWorkLogEntry(
        id: id,
        taskID: task.id,
        description: "Initial architecture and constraints",
        startedAt: startedAt,
        addedAt: addedAt,
        endedAt: endedAt,
        updatedAt: updatedAt)

    #expect(firstEntry == secondEntry)
}
