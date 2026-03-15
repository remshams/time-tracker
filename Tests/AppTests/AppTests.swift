import Testing

@testable import App

@MainActor
@Test func contentViewModuleLoads() {
    let repository = InMemoryTaskRepository(tasks: [
        makeTask(title: "Write project plan", description: "Capture the current decisions."),
        makeTask(title: "Review next step"),
    ])

    let view = ContentView(viewModel: TaskListViewModel(repository: repository))

    #expect(type(of: view) == ContentView.self)
}

@MainActor
@Test func taskListViewModuleLoads() {
    let repository = InMemoryTaskRepository(tasks: [
        makeTask(title: "Write project plan", description: "Capture the current decisions."),
        makeTask(title: "Review next step"),
    ])

    let view = TaskListView(viewModel: TaskListViewModel(repository: repository))

    #expect(type(of: view) == TaskListView.self)
}

private func makeTask(title: String, description: String? = nil) -> Task {
    do {
        return try Task(title: title, description: description)
    } catch {
        Issue.record("Failed to create test task: \(error)")
        fatalError("Failed to create test task: \(error)")
    }
}
