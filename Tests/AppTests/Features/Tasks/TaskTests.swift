import Testing

@testable import App

@Test func taskStoresTitleAndDescription() {
    let task = try! Task(title: "Write project plan", description: "Capture the implementation checkpoints.")
    let anotherTask = try! Task(title: "Another task")

    #expect(task.id != anotherTask.id)
    #expect(task.title == "Write project plan")
    #expect(task.description == "Capture the implementation checkpoints.")
}

@Test func taskKeepsExplicitIdentifier() {
    let identifier = try! Task(title: "Identifier seed").id
    let task = try! Task(id: identifier, title: "Write project plan")

    #expect(task.id == identifier)
}

@Test func taskAllowsMissingDescription() {
    let task = try! Task(title: "Review task list")

    #expect(task.title == "Review task list")
    #expect(task.description == nil)
}

@Test func taskUsesValueEquality() {
    let firstTask = try! Task(title: "Review task list", description: "Check each checkpoint.")
    let secondTask = try! Task(id: firstTask.id, title: "Review task list", description: "Check each checkpoint.")

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
