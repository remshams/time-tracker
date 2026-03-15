import Testing

@testable import App

@Test func taskStoresTitleAndDescription() {
    let task = makeTask(title: "Write project plan", description: "Capture the implementation checkpoints.")
    let anotherTask = makeTask(title: "Another task")

    #expect(task.id != anotherTask.id)
    #expect(task.title == "Write project plan")
    #expect(task.description == "Capture the implementation checkpoints.")
}

@Test func taskKeepsExplicitIdentifier() {
    let identifier = makeTask(title: "Identifier seed").id
    let task = makeTask(id: identifier, title: "Write project plan")

    #expect(task.id == identifier)
}

@Test func taskAllowsMissingDescription() {
    let task = makeTask(title: "Review task list")

    #expect(task.title == "Review task list")
    #expect(task.description == nil)
}

@Test func taskUsesValueEquality() {
    let firstTask = makeTask(title: "Review task list", description: "Check each checkpoint.")
    let secondTask = makeTask(id: firstTask.id, title: "Review task list", description: "Check each checkpoint.")

    #expect(firstTask == secondTask)
}

@Test func taskRejectsAnEmptyTitle() {
    #expect(throws: Task.ValidationError.emptyTitle) {
        try Task(title: "")
    }
}

@Test func taskRejectsAWhitespaceOnlyTitle() {
    #expect(throws: Task.ValidationError.emptyTitle) {
        try Task(title: "  \n  ")
    }
}

private func makeTask(id: Task.ID = .init(), title: String, description: String? = nil) -> Task {
    do {
        return try Task(id: id, title: title, description: description)
    } catch {
        Issue.record("Failed to create test task: \(error)")
        fatalError("Failed to create test task: \(error)")
    }
}
