import Testing
@testable import App

@Test func taskStoresTitleAndDescription() {
    let task = Task(title: "Write project plan", description: "Capture the implementation checkpoints.")
    let anotherTask = Task(title: "Another task")

    #expect(task.id != anotherTask.id)
    #expect(task.title == "Write project plan")
    #expect(task.description == "Capture the implementation checkpoints.")
}

@Test func taskKeepsExplicitIdentifier() {
    let identifier = Task(title: "Identifier seed").id
    let task = Task(id: identifier, title: "Write project plan")

    #expect(task.id == identifier)
}

@Test func taskAllowsMissingDescription() {
    let task = Task(title: "Review task list")

    #expect(task.title == "Review task list")
    #expect(task.description == nil)
}

@Test func taskUsesValueEquality() {
    let firstTask = Task(title: "Review task list", description: "Check each checkpoint.")
    let secondTask = Task(id: firstTask.id, title: "Review task list", description: "Check each checkpoint.")

    #expect(firstTask == secondTask)
}
