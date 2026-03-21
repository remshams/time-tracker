import Foundation
import Testing

@testable import App

@Test func workLogEntryStoresProvidedFields() {
    let task = makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_060)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_360)
    let updatedAt = Date(timeIntervalSince1970: 1_700_000_420)

    let entry = makeWorkLogEntry(
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
    let task = makeTask(title: "Write project plan")
    let identifier = UUID()

    let entry = makeWorkLogEntry(id: identifier, taskID: task.id)

    #expect(entry.id == identifier)
}

@Test func workLogEntryAllowsMissingDescription() {
    let task = makeTask(title: "Write project plan")

    let entry = makeWorkLogEntry(taskID: task.id)

    #expect(entry.description == nil)
}

@Test func workLogEntryAllowsMissingEndedAt() {
    let task = makeTask(title: "Write project plan")

    let entry = makeWorkLogEntry(taskID: task.id, endedAt: nil)

    #expect(entry.endedAt == nil)
    #expect(entry.duration == nil)
}

@Test func workLogEntryComputesDurationFromStartAndEnd() {
    let task = makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_900)

    let entry = makeWorkLogEntry(taskID: task.id, startedAt: startedAt, endedAt: endedAt)

    #expect(entry.duration == .seconds(900))
}

@Test func workLogEntryRejectsEndedAtEarlierThanStartedAt() {
    let task = makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_900)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_100)

    #expect(throws: WorkLogEntry.ValidationError.endedBeforeStarted) {
        try WorkLogEntry(taskID: task.id, startedAt: startedAt, endedAt: endedAt)
    }
}

@Test func workLogEntryRejectsUpdatedAtEarlierThanAddedAt() {
    let task = makeTask(title: "Write project plan")
    let startedAt = Date(timeIntervalSince1970: 1_700_000_100)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_300)
    let updatedAt = Date(timeIntervalSince1970: 1_700_000_200)

    #expect(throws: WorkLogEntry.ValidationError.updatedBeforeAdded) {
        try WorkLogEntry(taskID: task.id, startedAt: startedAt, addedAt: addedAt, updatedAt: updatedAt)
    }
}

@Test func workLogEntryUsesValueEquality() {
    let task = makeTask(title: "Write project plan")
    let id = UUID()
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let addedAt = Date(timeIntervalSince1970: 1_700_000_060)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_360)
    let updatedAt = Date(timeIntervalSince1970: 1_700_000_420)

    let firstEntry = makeWorkLogEntry(
        id: id,
        taskID: task.id,
        description: "Initial architecture and constraints",
        startedAt: startedAt,
        addedAt: addedAt,
        endedAt: endedAt,
        updatedAt: updatedAt)
    let secondEntry = makeWorkLogEntry(
        id: id,
        taskID: task.id,
        description: "Initial architecture and constraints",
        startedAt: startedAt,
        addedAt: addedAt,
        endedAt: endedAt,
        updatedAt: updatedAt)

    #expect(firstEntry == secondEntry)
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
            updatedAt: updatedAt)
    } catch {
        Issue.record("Failed to create test work log entry: \(error)")
        fatalError("Failed to create test work log entry: \(error)")
    }
}
